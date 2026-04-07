//! DreamForge HTTP/WebSocket server.
//!
//! Provides an Axum-based server with REST routes and WebSocket streaming
//! for the DreamForge browser UI. Wraps DreamForge's `ConversationRuntime`
//! via an async agent bridge.

pub mod agent_bridge;
pub mod config;
pub mod documents;
pub mod files;
pub mod memory_routes;
pub mod routes;
pub mod session_store;
pub mod voice;
pub mod ws;
mod ws_types;

pub use config::ServerConfig;
pub use ws_types::{WsIncoming, WsOutgoing, WsMessageType};

use std::sync::Arc;

use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

/// Shared application state accessible by all route handlers.
pub struct AppState {
    pub config: ServerConfig,
    pub sessions: session_store::SessionStore,
    /// Discovered model name (auto-detected or from config).
    pub discovered_model: std::sync::RwLock<String>,
    /// MCP server manager for tool discovery and execution.
    pub mcp_manager: Option<std::sync::Arc<std::sync::Mutex<runtime::McpServerManager>>>,
    /// Discovered MCP tools (cached from startup discovery).
    pub mcp_tools: Vec<runtime::ManagedMcpTool>,
    /// Shared memory store for CRUD routes and system prompt injection.
    pub memory_store: runtime::memory::MemoryStore,
    /// RAG pipeline (None if disabled or services unreachable).
    pub rag_pipeline: Option<std::sync::Arc<dreamforge_rag::pipeline::Pipeline>>,
}

/// Build the Axum router with all routes and middleware.
pub fn build_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut router = Router::new()
        .merge(routes::health_routes())
        .merge(routes::api_routes(Arc::clone(&state)))
        .merge(files::file_routes(Arc::clone(&state)))
        .merge(memory_routes::memory_routes(Arc::clone(&state)))
        .merge(documents::document_routes(Arc::clone(&state)))
        .merge(voice::voice_routes(Arc::clone(&state)))
        .merge(ws::ws_routes(Arc::clone(&state)));

    // Serve the frontend SPA from disk if available.
    let frontend_dir = resolve_frontend_dir();
    if let Some(dir) = frontend_dir {
        let index = dir.join("index.html");
        tracing::info!("serving frontend from {}", dir.display());
        router = router.fallback_service(
            ServeDir::new(&dir).fallback(ServeFile::new(index)),
        );
    } else {
        tracing::warn!("no frontend dist/ found — set DREAMFORGE_FRONTEND_DIR");
    }

    router.layer(cors)
}

/// Locate the frontend dist directory by checking, in order:
/// 1. `DREAMFORGE_FRONTEND_DIR` env var
/// 2. `frontend/dist/` relative to the current directory
/// 3. `../frontend/dist/` relative to the binary
fn resolve_frontend_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("DREAMFORGE_FRONTEND_DIR") {
        let p = std::path::PathBuf::from(dir);
        if p.join("index.html").exists() {
            return Some(p);
        }
    }
    let candidates = [
        std::path::PathBuf::from("frontend/dist"),
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("../frontend/dist"),
    ];
    candidates.into_iter().find(|p| p.join("index.html").exists())
}

/// Start the server on the given address.
///
/// # Errors
/// Returns an error if the server fails to bind or encounters a fatal I/O error.
pub async fn serve(mut config: ServerConfig) -> Result<(), Box<dyn std::error::Error>> {
    let addr = config.listen_addr();

    // Model auto-detection (Step 5)
    let discovered_model = auto_detect_model(&config).await;
    if config.model.is_empty() && !discovered_model.is_empty() {
        tracing::info!(model = %discovered_model, "auto-detected model from LLM endpoint");
        config.model = discovered_model.clone();
    }

    // Load persisted sessions (Step 3)
    let sessions_dir = session_store::SessionStore::ensure_sessions_dir(&config.data_dir);
    let sessions = if let Some(ref dir) = sessions_dir {
        session_store::SessionStore::load_from_disk(dir)
    } else {
        session_store::SessionStore::new()
    };

    // Crash recovery: reset any sessions stuck in Active status from a previous crash
    let mut recovered = 0;
    for session in sessions.list() {
        if session.status == session_store::SessionStatus::Active {
            let mut s = session.clone();
            s.status = session_store::SessionStatus::Errored;
            s.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            sessions.put(s);
            recovered += 1;
        }
    }
    if recovered > 0 {
        tracing::info!("crash recovery: reset {recovered} active session(s) to errored");
        if let Some(ref dir) = sessions_dir {
            sessions.save_all_to_disk(dir);
        }
    }

    // Initialize MCP servers from workspace config.
    // Use spawn_blocking because we're already inside the tokio runtime and
    // McpServerManager needs its own runtime for subprocess I/O.
    let workspace_for_mcp = config.workspace.clone();
    let (mcp_manager, mcp_tools) = tokio::task::spawn_blocking(move || {
        let config_loader = runtime::ConfigLoader::default_for(&workspace_for_mcp);
        let runtime_config = config_loader.load().unwrap_or_else(|e| {
            tracing::warn!("failed to load runtime config for MCP: {e}");
            runtime::RuntimeConfig::empty()
        });
        let mcp_config = runtime_config.mcp();
        if mcp_config.servers().is_empty() {
            tracing::info!("MCP: no servers configured");
            return (None, Vec::new());
        }

        let mcp_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create MCP discovery runtime");

        let mut manager = runtime::McpServerManager::from_servers(mcp_config.servers());
        let report = mcp_rt.block_on(manager.discover_tools_best_effort());
        tracing::info!(
            "MCP: {} tools discovered from {} servers ({} failed, {} unsupported)",
            report.tools.len(),
            mcp_config.servers().len(),
            report.failed_servers.len(),
            report.unsupported_servers.len(),
        );
        for tool in &report.tools {
            tracing::info!("  MCP tool: {}", tool.qualified_name);
        }
        let tools = report.tools;
        (
            Some(Arc::new(std::sync::Mutex::new(manager))),
            tools,
        )
    })
    .await
    .unwrap_or_else(|e| {
        tracing::warn!("MCP discovery task failed: {e}");
        (None, Vec::new())
    });

    // Initialize memory store
    let memory_dir = std::path::PathBuf::from(&config.data_dir).join("memory");
    let memory_store = runtime::memory::MemoryStore::new(&memory_dir);

    // Initialize RAG pipeline (graceful — None if disabled or services down)
    let rag_pipeline = if config.rag_enabled {
        let embeddings = dreamforge_rag::embeddings::EmbeddingsClient::new(&config.embeddings_url);
        let vector_store = dreamforge_rag::vector_store::VectorStore::new(&config.qdrant_url);
        if embeddings.health_check().await {
            tracing::info!("RAG pipeline enabled (qdrant={}, embeddings={})", config.qdrant_url, config.embeddings_url);
            Some(Arc::new(dreamforge_rag::pipeline::Pipeline::new(embeddings, vector_store)))
        } else {
            tracing::warn!("RAG disabled: embeddings service unreachable at {}", config.embeddings_url);
            None
        }
    } else {
        None
    };

    let state = Arc::new(AppState {
        config,
        sessions,
        discovered_model: std::sync::RwLock::new(discovered_model),
        mcp_manager,
        mcp_tools,
        memory_store,
        rag_pipeline,
    });

    let app = build_router(Arc::clone(&state));
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("DreamForge server listening on {addr}");

    // Graceful shutdown: save sessions when server stops
    let shutdown_state = Arc::clone(&state);
    let server = axum::serve(listener, app).with_graceful_shutdown(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down — saving sessions");
        if let Some(dir) = session_store::SessionStore::ensure_sessions_dir(
            &shutdown_state.config.data_dir,
        ) {
            shutdown_state.sessions.save_all_to_disk(&dir);
        }
    });

    server.await?;
    Ok(())
}

/// Query the LLM API's /models endpoint to discover the available model.
async fn auto_detect_model(config: &ServerConfig) -> String {
    if !config.model.is_empty() {
        return config.model.clone();
    }

    let url = format!("{}/models", config.llm_api_url.trim_end_matches('/'));
    tracing::info!("auto-detecting model from {url}");

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("model auto-detection failed: {e}");
            return String::new();
        }
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("model auto-detection parse error: {e}");
            return String::new();
        }
    };

    // OpenAI-compatible format: { "data": [{ "id": "model-name" }] }
    if let Some(models) = json["data"].as_array() {
        if let Some(first) = models.first() {
            if let Some(id) = first["id"].as_str() {
                return id.to_string();
            }
        }
    }

    // Ollama format: { "models": [{ "name": "model-name" }] }
    if let Some(models) = json["models"].as_array() {
        if let Some(first) = models.first() {
            if let Some(name) = first["name"].as_str() {
                return name.to_string();
            }
        }
    }

    String::new()
}
