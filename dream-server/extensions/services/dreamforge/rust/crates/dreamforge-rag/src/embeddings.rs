//! Client for Text-Embeddings-Inference (TEI) service.

use serde::Serialize;

/// TEI embeddings client.
#[derive(Debug, Clone)]
pub struct EmbeddingsClient {
    http: reqwest::Client,
    base_url: String,
    batch_size: usize,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
}


/// Error from embedding operations.
#[derive(Debug)]
pub enum EmbeddingsError {
    Http(reqwest::Error),
    EmptyResponse,
}

impl From<reqwest::Error> for EmbeddingsError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl std::fmt::Display for EmbeddingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "embeddings HTTP error: {e}"),
            Self::EmptyResponse => write!(f, "embeddings service returned empty response"),
        }
    }
}

impl EmbeddingsClient {
    /// Create a new client pointing at a TEI service.
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            batch_size: 32,
        }
    }

    /// Generate embeddings for a list of texts, automatically batching.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingsError> {
        let mut all_vectors = Vec::with_capacity(texts.len());

        for batch in texts.chunks(self.batch_size) {
            let resp: Vec<Vec<f32>> = self
                .http
                .post(format!("{}/embed", self.base_url))
                .json(&EmbedRequest { inputs: batch })
                .send()
                .await?
                .json()
                .await?;
            all_vectors.extend(resp);
        }

        Ok(all_vectors)
    }

    /// Probe the service to determine vector dimensionality.
    pub async fn get_vector_size(&self) -> Result<usize, EmbeddingsError> {
        let vectors = self.embed(&["test".to_string()]).await?;
        vectors
            .first()
            .map(Vec::len)
            .ok_or(EmbeddingsError::EmptyResponse)
    }

    /// Health check.
    pub async fn health_check(&self) -> bool {
        self.http
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_construction() {
        let client = EmbeddingsClient::new("http://localhost:80/");
        assert_eq!(client.base_url, "http://localhost:80");
        assert_eq!(client.batch_size, 32);
    }
}
