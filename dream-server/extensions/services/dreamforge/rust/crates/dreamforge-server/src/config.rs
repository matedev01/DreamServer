//! Server configuration from environment variables.

/// Configuration for the DreamForge server.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub api_key: Option<String>,
    pub workspace: String,
    pub llm_api_url: String,
    pub model: String,
    pub permission_mode: String,
    pub max_turns: usize,
    pub data_dir: String,
    pub compact_threshold: u32,
    pub compact_preserve: usize,
    pub qdrant_url: String,
    pub embeddings_url: String,
    pub rag_enabled: bool,
}

impl ServerConfig {
    /// Load configuration from environment variables with sensible defaults.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            host: env_or("DREAMFORGE_HOST", "0.0.0.0"),
            port: env_or("DREAMFORGE_PORT", "3010")
                .parse()
                .unwrap_or(3010),
            api_key: std::env::var("DREAMFORGE_API_KEY").ok().filter(|s| !s.is_empty()),
            workspace: env_or("DREAMFORGE_WORKSPACE", "."),
            llm_api_url: env_or("LLM_API_URL", "http://localhost:11434"),
            model: env_or("DREAMFORGE_MODEL", ""),
            permission_mode: env_or("DREAMFORGE_PERMISSION_MODE", "default"),
            max_turns: env_or("DREAMFORGE_MAX_TURNS", "200")
                .parse()
                .unwrap_or(200),
            data_dir: env_or("DREAMFORGE_DATA_DIR", "/data/dreamforge"),
            compact_threshold: env_or("DREAMFORGE_COMPACT_THRESHOLD", "10000")
                .parse()
                .unwrap_or(10_000),
            compact_preserve: env_or("DREAMFORGE_COMPACT_PRESERVE", "4")
                .parse()
                .unwrap_or(4),
            qdrant_url: env_or("QDRANT_URL", "http://localhost:6333"),
            embeddings_url: env_or("EMBEDDINGS_URL", "http://localhost:8090"),
            rag_enabled: env_or("DREAMFORGE_RAG_ENABLED", "false")
                .parse()
                .unwrap_or(false),
        }
    }

    /// Returns the socket address string.
    #[must_use]
    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Validate the API key from a request. Returns `true` if:
    /// - No API key is configured (open access), or
    /// - The provided key matches the configured key.
    #[must_use]
    pub fn check_auth(&self, provided: Option<&str>) -> bool {
        match &self.api_key {
            None => true,
            Some(expected) => provided.is_some_and(|k| k == expected),
        }
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_auth_allows_when_no_key_configured() {
        let config = ServerConfig {
            api_key: None,
            host: String::new(),
            port: 3010,
            workspace: String::new(),
            llm_api_url: String::new(),
            model: String::new(),
            permission_mode: String::new(),
            max_turns: 200,
            data_dir: String::new(),
            compact_threshold: 10_000,
            compact_preserve: 4,
            qdrant_url: String::new(),
            embeddings_url: String::new(),
            rag_enabled: false,
        };
        assert!(config.check_auth(None));
        assert!(config.check_auth(Some("anything")));
    }

    #[test]
    fn check_auth_requires_matching_key() {
        let config = ServerConfig {
            api_key: Some("secret123".to_string()),
            host: String::new(),
            port: 3010,
            workspace: String::new(),
            llm_api_url: String::new(),
            model: String::new(),
            permission_mode: String::new(),
            max_turns: 200,
            data_dir: String::new(),
            compact_threshold: 10_000,
            compact_preserve: 4,
            qdrant_url: String::new(),
            embeddings_url: String::new(),
            rag_enabled: false,
        };
        assert!(!config.check_auth(None));
        assert!(!config.check_auth(Some("wrong")));
        assert!(config.check_auth(Some("secret123")));
    }
}
