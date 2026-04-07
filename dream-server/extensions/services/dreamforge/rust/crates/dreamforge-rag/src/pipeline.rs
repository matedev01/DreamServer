//! RAG pipeline orchestrator: ingest documents and search.

use sha2::{Digest, Sha256};
use serde_json::json;
use tracing::info;

use crate::chunker;
use crate::embeddings::{EmbeddingsClient, EmbeddingsError};
use crate::vector_store::{SearchResult, UpsertPoint, VectorStore, VectorStoreError};

/// Result of ingesting a document.
#[derive(Debug)]
pub struct IngestResult {
    pub doc_id: String,
    pub chunk_count: usize,
    pub collection: String,
    pub filename: String,
}

/// Error from pipeline operations.
#[derive(Debug)]
pub enum PipelineError {
    Embeddings(EmbeddingsError),
    VectorStore(VectorStoreError),
    EmptyDocument,
}

impl From<EmbeddingsError> for PipelineError {
    fn from(e: EmbeddingsError) -> Self {
        Self::Embeddings(e)
    }
}

impl From<VectorStoreError> for PipelineError {
    fn from(e: VectorStoreError) -> Self {
        Self::VectorStore(e)
    }
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Embeddings(e) => write!(f, "embedding error: {e}"),
            Self::VectorStore(e) => write!(f, "vector store error: {e}"),
            Self::EmptyDocument => write!(f, "document is empty"),
        }
    }
}

/// RAG pipeline orchestrating chunking, embedding, and vector storage.
pub struct Pipeline {
    embeddings: EmbeddingsClient,
    vector_store: VectorStore,
    chunk_size: usize,
    chunk_overlap: usize,
}

impl Pipeline {
    #[must_use]
    pub fn new(embeddings: EmbeddingsClient, vector_store: VectorStore) -> Self {
        Self {
            embeddings,
            vector_store,
            chunk_size: chunker::DEFAULT_CHUNK_SIZE,
            chunk_overlap: chunker::DEFAULT_OVERLAP,
        }
    }

    #[must_use]
    pub fn with_chunk_config(mut self, size: usize, overlap: usize) -> Self {
        self.chunk_size = size;
        self.chunk_overlap = overlap;
        self
    }

    /// Ingest a document: chunk, embed, store.
    pub async fn ingest(
        &self,
        collection: &str,
        filename: &str,
        content: &str,
    ) -> Result<IngestResult, PipelineError> {
        if content.is_empty() {
            return Err(PipelineError::EmptyDocument);
        }

        let doc_id = content_hash(content);
        let chunks = chunker::chunk_text(content, self.chunk_size, self.chunk_overlap);

        if chunks.is_empty() {
            return Err(PipelineError::EmptyDocument);
        }

        info!(
            doc_id = %doc_id,
            chunks = chunks.len(),
            filename = %filename,
            "ingesting document"
        );

        // Get vector size and ensure collection
        let vector_size = self.embeddings.get_vector_size().await?;
        self.vector_store
            .ensure_collection(collection, vector_size)
            .await?;

        // Embed all chunks
        let vectors = self.embeddings.embed(&chunks).await?;

        // Build upsert points
        let points: Vec<UpsertPoint> = chunks
            .iter()
            .zip(vectors.iter())
            .enumerate()
            .map(|(i, (text, vector))| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs_f64();

                UpsertPoint {
                    id: format!("{}_{i:04}", doc_id),
                    vector: vector.clone(),
                    payload: json!({
                        "doc_id": doc_id,
                        "filename": filename,
                        "chunk_index": i,
                        "text": text,
                        "ingested_at": now,
                    }),
                }
            })
            .collect();

        self.vector_store.upsert(collection, points).await?;

        Ok(IngestResult {
            doc_id,
            chunk_count: chunks.len(),
            collection: collection.to_string(),
            filename: filename.to_string(),
        })
    }

    /// Search for relevant chunks.
    pub async fn search(
        &self,
        collection: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>, PipelineError> {
        let query_vec = self.embeddings.embed(&[query.to_string()]).await?;
        let vector = query_vec
            .first()
            .ok_or(PipelineError::Embeddings(EmbeddingsError::EmptyResponse))?;

        let results = self
            .vector_store
            .search(collection, vector, limit, 0.3)
            .await?;

        Ok(results)
    }

    /// Delete a document from a collection.
    pub async fn delete_document(
        &self,
        collection: &str,
        doc_id: &str,
    ) -> Result<(), PipelineError> {
        self.vector_store
            .delete_by_doc_id(collection, doc_id)
            .await?;
        Ok(())
    }
}

/// SHA-256 hash of content, truncated to 16 hex chars (stable document ID).
fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 8 bytes = 16 hex chars
}

/// Minimal hex encoder (avoids pulling in the `hex` crate).
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_is_stable() {
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16);
    }

    #[test]
    fn content_hash_differs_for_different_input() {
        let h1 = content_hash("hello");
        let h2 = content_hash("world");
        assert_ne!(h1, h2);
    }
}
