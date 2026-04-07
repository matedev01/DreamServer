//! File I/O routes: read, save, and tree.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

use crate::AppState;

const MAX_FILE_SIZE: u64 = 5 * 1024 * 1024; // 5 MB
const MAX_TREE_ENTRIES: usize = 100;
const MAX_TREE_DEPTH: usize = 5;

const SKIP_DIRS: &[&str] = &[".git", "node_modules", "__pycache__", "target", ".venv"];

pub fn file_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/files/read", get(read_file))
        .route("/api/files/save", post(save_file))
        .route("/api/files/tree", get(file_tree))
        .with_state(state)
}

// ---------- path validation ----------

fn validate_workspace_path(path: &str, workspace: &str) -> Result<PathBuf, StatusCode> {
    let resolved = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let workspace_canonical = PathBuf::from(workspace)
        .canonicalize()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !resolved.starts_with(&workspace_canonical) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(resolved)
}

fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("py") => "python",
        Some("js") | Some("jsx") => "javascript",
        Some("ts") | Some("tsx") => "typescript",
        Some("json") => "json",
        Some("md") => "markdown",
        Some("toml") => "toml",
        Some("yaml" | "yml") => "yaml",
        Some("html" | "htm") => "html",
        Some("css") => "css",
        Some("sh" | "bash") => "shell",
        Some("sql") => "sql",
        Some("xml") => "xml",
        Some("go") => "go",
        Some("java") => "java",
        Some("c" | "h") => "c",
        Some("cpp" | "hpp" | "cc") => "cpp",
        Some("dockerfile") => "dockerfile",
        _ => "text",
    }
}

// ---------- GET /api/files/read ----------

#[derive(Deserialize)]
struct ReadQuery {
    path: String,
}

async fn read_file(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ReadQuery>,
) -> impl IntoResponse {
    let resolved = match validate_workspace_path(&query.path, &state.config.workspace) {
        Ok(p) => p,
        Err(StatusCode::FORBIDDEN) => {
            return (StatusCode::FORBIDDEN, Json(json!({"error": "path outside workspace"}))).into_response();
        }
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "file not found"}))).into_response();
        }
    };

    let meta = match std::fs::metadata(&resolved) {
        Ok(m) => m,
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "file not found"}))).into_response();
        }
    };

    if !meta.is_file() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "not a file"}))).into_response();
    }

    if meta.len() > MAX_FILE_SIZE {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "file too large (max 5MB)"}))).into_response();
    }

    let content = match std::fs::read_to_string(&resolved) {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "failed to read file"}))).into_response();
        }
    };

    let name = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    (StatusCode::OK, Json(json!({
        "path": resolved.display().to_string(),
        "name": name,
        "content": content,
        "language": detect_language(&resolved),
        "size": meta.len(),
    }))).into_response()
}

// ---------- POST /api/files/save ----------

#[derive(Deserialize)]
struct SaveRequest {
    path: String,
    content: String,
}

async fn save_file(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveRequest>,
) -> impl IntoResponse {
    // For save, the file may not exist yet — validate the parent directory
    let target = PathBuf::from(&body.path);
    let workspace_canonical = match PathBuf::from(&state.config.workspace).canonicalize() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "workspace error"}))),
    };

    // Check parent exists and is within workspace
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // After creating parents, validate the path
    let parent_canonical = target
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_default();

    if !parent_canonical.starts_with(&workspace_canonical) {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "path outside workspace"})));
    }

    match std::fs::write(&target, &body.content) {
        Ok(()) => {
            let size = body.content.len();
            (StatusCode::OK, Json(json!({"path": body.path, "size": size, "saved": true})))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": format!("write failed: {e}")}))),
    }
}

// ---------- GET /api/files/tree ----------

#[derive(Deserialize)]
struct TreeQuery {
    path: Option<String>,
    depth: Option<usize>,
}

async fn file_tree(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TreeQuery>,
) -> impl IntoResponse {
    let root_path = query
        .path
        .as_deref()
        .unwrap_or(&state.config.workspace);
    let max_depth = query.depth.unwrap_or(3).min(MAX_TREE_DEPTH);

    let root = PathBuf::from(root_path);
    let root_canonical = match root.canonicalize() {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, Json(json!({"error": "path not found"}))),
    };

    let workspace_canonical = match PathBuf::from(&state.config.workspace).canonicalize() {
        Ok(w) => w,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "workspace error"}))),
    };

    if !root_canonical.starts_with(&workspace_canonical) {
        return (StatusCode::FORBIDDEN, Json(json!({"error": "path outside workspace"})));
    }

    let name = root_canonical
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace")
        .to_string();

    let children = build_tree(&root_canonical, 0, max_depth);

    (StatusCode::OK, Json(json!({
        "root": root_canonical.display().to_string(),
        "name": name,
        "children": children,
    })))
}

fn build_tree(dir: &Path, current_depth: usize, max_depth: usize) -> Vec<serde_json::Value> {
    if current_depth >= max_depth {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in entries.flatten().take(MAX_TREE_ENTRIES) {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        if path.is_dir() {
            let children = build_tree(&path, current_depth + 1, max_depth);
            dirs.push(json!({
                "name": name,
                "path": path.display().to_string(),
                "type": "directory",
                "children": children,
            }));
        } else {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            files.push(json!({
                "name": name,
                "path": path.display().to_string(),
                "type": "file",
                "size": size,
            }));
        }
    }

    // Sort: directories first (alphabetical), then files (alphabetical)
    dirs.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    dirs.extend(files);
    dirs
}
