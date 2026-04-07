//! RAG document routes: upload, search, collections, delete.

use std::sync::Arc;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

pub fn document_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/documents/upload", post(upload_document))
        .route("/api/documents/search", post(search_documents))
        .route("/api/documents/collections", get(list_collections))
        .route(
            "/api/documents/collections/{name}/documents/{doc_id}",
            delete(delete_document),
        )
        .with_state(state)
}

async fn upload_document(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let pipeline = match &state.rag_pipeline {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "RAG pipeline not enabled"}))).into_response(),
    };

    let mut collection = "default".to_string();
    let mut filename = String::new();
    let mut content = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "collection" => {
                collection = field.text().await.unwrap_or_else(|_| "default".to_string());
            }
            "file" => {
                filename = field
                    .file_name()
                    .unwrap_or("uploaded.txt")
                    .to_string();
                content = field.text().await.unwrap_or_default();
            }
            _ => {}
        }
    }

    if content.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "no file content"}))).into_response();
    }

    match pipeline.ingest(&collection, &filename, &content).await {
        Ok(result) => (StatusCode::OK, Json(json!({
            "doc_id": result.doc_id,
            "chunk_count": result.chunk_count,
            "collection": result.collection,
            "filename": result.filename,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("ingest failed: {e}")})))
            .into_response(),
    }
}

#[derive(Deserialize)]
struct SearchRequest {
    query: String,
    #[serde(default = "default_collection")]
    collection: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_collection() -> String {
    "default".to_string()
}

fn default_limit() -> usize {
    5
}

async fn search_documents(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SearchRequest>,
) -> impl IntoResponse {
    let pipeline = match &state.rag_pipeline {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "RAG pipeline not enabled"}))).into_response(),
    };

    match pipeline.search(&body.collection, &body.query, body.limit).await {
        Ok(results) => (StatusCode::OK, Json(json!({
            "results": results,
            "count": results.len(),
            "collection": body.collection,
            "query": body.query,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("search failed: {e}")})))
            .into_response(),
    }
}

async fn list_collections(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    if state.rag_pipeline.is_none() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "RAG pipeline not enabled"}))).into_response();
    }

    // Proxy directly to Qdrant's collections endpoint
    let url = format!("{}/collections", state.config.qdrant_url.trim_end_matches('/'));
    match reqwest::get(&url).await {
        Ok(resp) => {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or(json!({"result": {"collections": []}}));
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("qdrant unreachable: {e}")})))
            .into_response(),
    }
}

async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path((collection, doc_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let pipeline = match &state.rag_pipeline {
        Some(p) => p,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error": "RAG pipeline not enabled"}))).into_response(),
    };

    match pipeline.delete_document(&collection, &doc_id).await {
        Ok(()) => (StatusCode::OK, Json(json!({"deleted": true, "doc_id": doc_id, "collection": collection}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("delete failed: {e}")})))
            .into_response(),
    }
}
