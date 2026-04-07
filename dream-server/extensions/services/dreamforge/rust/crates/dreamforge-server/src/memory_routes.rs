//! Memory CRUD routes.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

pub fn memory_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/memory", get(list_memories))
        .route("/api/memory", post(create_memory))
        .route("/api/memory/{id}", get(get_memory))
        .route("/api/memory/{id}", put(update_memory))
        .route("/api/memory/{id}", delete(delete_memory))
        .with_state(state)
}

async fn list_memories(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let entries = state.memory_store.load_all();
    let count = entries.len();
    Json(json!({ "entries": entries, "count": count }))
}

async fn get_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.memory_store.get(&id) {
        Some(entry) => (StatusCode::OK, Json(json!(entry))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "memory not found"}))).into_response(),
    }
}

#[derive(Deserialize)]
struct CreateMemoryRequest {
    #[serde(rename = "type")]
    memory_type: String,
    title: String,
    content: String,
    #[serde(default)]
    description: String,
}

async fn create_memory(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateMemoryRequest>,
) -> impl IntoResponse {
    let memory_type = match body.memory_type.as_str() {
        "user" => runtime::memory::MemoryType::User,
        "feedback" => runtime::memory::MemoryType::Feedback,
        "project" => runtime::memory::MemoryType::Project,
        "reference" => runtime::memory::MemoryType::Reference,
        _ => {
            return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid type, must be: user, feedback, project, or reference"}))).into_response();
        }
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let id = uuid::Uuid::new_v4().to_string().replace('-', "")[..12].to_string();

    let entry = runtime::memory::MemoryEntry {
        id: id.clone(),
        memory_type,
        title: body.title,
        description: body.description,
        content: body.content,
        file_path: None,
        created_at: now,
        updated_at: now,
        relevance_score: None,
    };

    match state.memory_store.put(&entry) {
        Ok(path) => (StatusCode::CREATED, Json(json!({
            "id": id,
            "path": path.display().to_string(),
            "created": true,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("failed to save: {e}")}))).into_response(),
    }
}

#[derive(Deserialize)]
struct UpdateMemoryRequest {
    title: Option<String>,
    content: Option<String>,
    description: Option<String>,
}

async fn update_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateMemoryRequest>,
) -> impl IntoResponse {
    let existing = match state.memory_store.get(&id) {
        Some(e) => e,
        None => return (StatusCode::NOT_FOUND, Json(json!({"error": "memory not found"}))).into_response(),
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let updated = runtime::memory::MemoryEntry {
        id: existing.id,
        memory_type: existing.memory_type,
        title: body.title.unwrap_or(existing.title),
        description: body.description.unwrap_or(existing.description),
        content: body.content.unwrap_or(existing.content),
        file_path: existing.file_path,
        created_at: existing.created_at,
        updated_at: now,
        relevance_score: None,
    };

    match state.memory_store.put(&updated) {
        Ok(_) => (StatusCode::OK, Json(json!({"id": id, "updated": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("failed to update: {e}")}))).into_response(),
    }
}

async fn delete_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.memory_store.delete(&id) {
        Json(json!({"deleted": true, "id": id}))
    } else {
        Json(json!({"deleted": false, "error": "memory not found"}))
    }
}
