//! Qdrant vector store client (REST API).

use serde::{Deserialize, Serialize};
use serde_json::json;

/// Qdrant REST client.
#[derive(Debug, Clone)]
pub struct VectorStore {
    http: reqwest::Client,
    base_url: String,
}

/// A search result from Qdrant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub text: String,
    pub filename: String,
    pub doc_id: String,
    pub chunk_index: u32,
}

/// Error from vector store operations.
#[derive(Debug)]
pub enum VectorStoreError {
    Http(reqwest::Error),
    Api(String),
}

impl From<reqwest::Error> for VectorStoreError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl std::fmt::Display for VectorStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "qdrant HTTP error: {e}"),
            Self::Api(msg) => write!(f, "qdrant API error: {msg}"),
        }
    }
}

impl VectorStore {
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Ensure a collection exists with the given vector size.
    pub async fn ensure_collection(
        &self,
        name: &str,
        vector_size: usize,
    ) -> Result<(), VectorStoreError> {
        let resp = self
            .http
            .put(format!("{}/collections/{name}", self.base_url))
            .json(&json!({
                "vectors": {
                    "size": vector_size,
                    "distance": "Cosine"
                }
            }))
            .send()
            .await?;

        // 409 Conflict = already exists, which is fine
        if resp.status().is_success() || resp.status().as_u16() == 409 {
            Ok(())
        } else {
            Err(VectorStoreError::Api(format!(
                "failed to create collection: {}",
                resp.status()
            )))
        }
    }

    /// Upsert points into a collection.
    pub async fn upsert(
        &self,
        collection: &str,
        points: Vec<UpsertPoint>,
    ) -> Result<(), VectorStoreError> {
        let points_json: Vec<serde_json::Value> = points
            .into_iter()
            .map(|p| {
                json!({
                    "id": p.id,
                    "vector": p.vector,
                    "payload": p.payload,
                })
            })
            .collect();

        let resp = self
            .http
            .put(format!(
                "{}/collections/{collection}/points",
                self.base_url
            ))
            .json(&json!({ "points": points_json }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(VectorStoreError::Api(format!(
                "upsert failed: {}",
                resp.status()
            )))
        }
    }

    /// Search for similar vectors.
    pub async fn search(
        &self,
        collection: &str,
        vector: &[f32],
        limit: usize,
        score_threshold: f32,
    ) -> Result<Vec<SearchResult>, VectorStoreError> {
        #[derive(Deserialize)]
        struct QdrantResult {
            result: Vec<QdrantHit>,
        }
        #[derive(Deserialize)]
        struct QdrantHit {
            id: serde_json::Value,
            score: f32,
            payload: Option<serde_json::Value>,
        }

        let resp: QdrantResult = self
            .http
            .post(format!(
                "{}/collections/{collection}/points/search",
                self.base_url
            ))
            .json(&json!({
                "vector": vector,
                "limit": limit,
                "score_threshold": score_threshold,
                "with_payload": true,
            }))
            .send()
            .await?
            .json()
            .await?;

        Ok(resp
            .result
            .into_iter()
            .map(|hit| {
                let payload = hit.payload.unwrap_or_default();
                SearchResult {
                    id: hit.id.to_string(),
                    score: hit.score,
                    text: payload["text"].as_str().unwrap_or_default().to_string(),
                    filename: payload["filename"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    doc_id: payload["doc_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    chunk_index: payload["chunk_index"].as_u64().unwrap_or(0) as u32,
                }
            })
            .collect())
    }

    /// Delete all points for a given doc_id.
    pub async fn delete_by_doc_id(
        &self,
        collection: &str,
        doc_id: &str,
    ) -> Result<(), VectorStoreError> {
        let resp = self
            .http
            .post(format!(
                "{}/collections/{collection}/points/delete",
                self.base_url
            ))
            .json(&json!({
                "filter": {
                    "must": [{"key": "doc_id", "match": {"value": doc_id}}]
                }
            }))
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(VectorStoreError::Api(format!(
                "delete failed: {}",
                resp.status()
            )))
        }
    }
}

/// A point to upsert into Qdrant.
pub struct UpsertPoint {
    pub id: String,
    pub vector: Vec<f32>,
    pub payload: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_construction() {
        let store = VectorStore::new("http://qdrant:6333/");
        assert_eq!(store.base_url, "http://qdrant:6333");
    }
}
