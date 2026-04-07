//! HTTP REST routes: health, bootstrap, sessions.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::AppState;

// ---------- health ----------

pub fn health_routes() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
}

async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "service": "dreamforge",
    }))
}

async fn readyz() -> impl IntoResponse {
    // TODO(Phase 3+): check LLM reachability
    Json(json!({
        "status": "ready",
    }))
}

// ---------- API routes ----------

pub fn api_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/bootstrap", get(bootstrap))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions", post(create_session))
        .route("/api/sessions/{session_id}", get(get_session))
        .route("/api/sessions/{session_id}", delete(delete_session))
        .route("/api/sessions/{session_id}/fork", post(fork_session))
        .route("/api/sessions/{session_id}/usage", get(session_usage))
        .route("/api/models", get(list_models))
        .route("/api/models/current", get(current_model))
        .with_state(state)
}

async fn bootstrap(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let discovered = state
        .discovered_model
        .read()
        .map(|m| m.clone())
        .unwrap_or_default();
    Json(json!({
        "model": state.config.model,
        "discovered_model": discovered,
        "workspace": state.config.workspace,
        "permission_mode": state.config.permission_mode,
        "max_turns": state.config.max_turns,
    }))
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sessions = state.sessions.list();
    Json(json!({ "sessions": sessions }))
}

#[derive(serde::Deserialize)]
struct CreateSessionRequest {
    working_directory: Option<String>,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let wd = body
        .working_directory
        .unwrap_or_else(|| state.config.workspace.clone());
    let session = state.sessions.create(&wd, &state.config.permission_mode);
    (StatusCode::CREATED, Json(json!(session)))
}

async fn get_session(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.sessions.get(&session_id) {
        Some(session) => (StatusCode::OK, Json(json!(session))),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        ),
    }
}

#[derive(serde::Deserialize)]
struct ForkRequest {
    branch_name: Option<String>,
}

async fn fork_session(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<ForkRequest>,
) -> impl IntoResponse {
    match state.sessions.get(&session_id) {
        Some(source) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let forked = crate::session_store::Session {
                id: uuid::Uuid::new_v4().to_string().replace('-', ""),
                status: crate::session_store::SessionStatus::Idle,
                created_at: now,
                updated_at: now,
                working_directory: source.working_directory.clone(),
                model_id: source.model_id.clone(),
                permission_mode: source.permission_mode.clone(),
                turn_count: source.turn_count,
                total_tokens_in: source.total_tokens_in,
                total_tokens_out: source.total_tokens_out,
                title: body.branch_name.as_ref().map(|n| format!("Fork: {n}")).or(source.title.clone()),
                messages: source.messages.clone(),
            };
            let forked_id = forked.id.clone();
            state.sessions.put(forked);
            (
                StatusCode::CREATED,
                Json(json!({
                    "id": forked_id,
                    "forked_from": session_id,
                    "branch_name": body.branch_name,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        ),
    }
}

async fn session_usage(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.sessions.get(&session_id) {
        Some(session) => {
            // Estimate cost using runtime pricing
            let model = state
                .discovered_model
                .read()
                .map(|m| m.clone())
                .unwrap_or_default();
            let pricing = runtime::pricing_for_model(&model);
            let estimated_cost = pricing.map(|p| {
                let input_cost = session.total_tokens_in as f64 * p.input_cost_per_million / 1_000_000.0;
                let output_cost = session.total_tokens_out as f64 * p.output_cost_per_million / 1_000_000.0;
                input_cost + output_cost
            });
            (
                StatusCode::OK,
                Json(json!({
                    "session_id": session_id,
                    "tokens_in": session.total_tokens_in,
                    "tokens_out": session.total_tokens_out,
                    "turn_count": session.turn_count,
                    "estimated_cost": estimated_cost,
                    "model": model,
                })),
            )
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        ),
    }
}

async fn delete_session(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if state.sessions.delete(&session_id) {
        Json(json!({"deleted": true, "id": session_id}))
    } else {
        Json(json!({"deleted": false, "error": "session not found"}))
    }
}

// ---------- models ----------

async fn list_models(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let url = format!("{}/models", state.config.llm_api_url.trim_end_matches('/'));
    match reqwest::get(&url).await {
        Ok(resp) => {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or(json!({"data": []}));
            Json(body)
        }
        Err(_) => Json(json!({"data": [], "error": "LLM server unreachable"})),
    }
}

async fn current_model(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let model = state
        .discovered_model
        .read()
        .map(|m| m.clone())
        .unwrap_or_default();
    Json(json!({
        "model_name": model,
        "permission_mode": state.config.permission_mode,
    }))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt as _;

    use super::*;
    use crate::config::ServerConfig;
    use crate::session_store::SessionStore;

    fn test_state() -> Arc<AppState> {
        let tmp = std::env::temp_dir().join("dreamforge-test-memory");
        Arc::new(AppState {
            config: ServerConfig {
                host: "127.0.0.1".into(),
                port: 3010,
                api_key: None,
                workspace: "/workspace".into(),
                llm_api_url: "http://localhost:11434".into(),
                model: "test-model".into(),
                permission_mode: "default".into(),
                max_turns: 200,
                data_dir: String::new(),
                compact_threshold: 10_000,
                compact_preserve: 4,
                qdrant_url: String::new(),
                embeddings_url: String::new(),
                rag_enabled: false,
            },
            sessions: SessionStore::new(),
            discovered_model: std::sync::RwLock::new("test-model".into()),
            mcp_manager: None,
            mcp_tools: Vec::new(),
            memory_store: runtime::memory::MemoryStore::new(&tmp),
            rag_pipeline: None,
        })
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let app = health_routes().into_service();
        let resp = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn bootstrap_returns_config() {
        let state = test_state();
        let app = api_routes(state).into_service();
        let resp = app
            .oneshot(
                Request::get("/api/bootstrap")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["model"], "test-model");
    }

    #[tokio::test]
    async fn session_crud_lifecycle() {
        let state = test_state();
        let app = api_routes(Arc::clone(&state));

        // Create
        let resp = app
            .clone()
            .into_service()
            .oneshot(
                Request::post("/api/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"working_directory":"/tmp"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), 4096)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        // Get
        let resp = app
            .clone()
            .into_service()
            .oneshot(
                Request::get(&format!("/api/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // List
        let resp = app
            .clone()
            .into_service()
            .oneshot(
                Request::get("/api/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Delete
        let resp = app
            .into_service()
            .oneshot(
                Request::delete(&format!("/api/sessions/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
