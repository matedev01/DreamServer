//! WebSocket endpoint and message dispatch.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Query, State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::session_store::SessionStatus;
use crate::ws_types::{WsIncoming, WsMessageType, WsOutgoing};
use crate::AppState;

/// WebSocket query parameters.
#[derive(Deserialize)]
struct WsParams {
    token: Option<String>,
}

/// Build WebSocket route.
pub fn ws_routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

async fn ws_upgrade(
    State(state): State<Arc<AppState>>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Auth check
    if !state.config.check_auth(params.token.as_deref()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| handle_ws(socket, state))
        .into_response()
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let seq = Arc::new(AtomicU64::new(0));

    // Channel for sending messages back to the client from async tasks
    let (tx, mut rx) = mpsc::unbounded_channel::<WsOutgoing>();

    // Writer task: forwards channel messages to the WebSocket
    let write_task = tokio::spawn(async move {
        use futures_util::SinkExt;
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Load or create session
    let session = state
        .sessions
        .get_or_create(&state.config.workspace, &state.config.permission_mode);
    let session_id = session.id.clone();

    // Send initial session info (includes discovered model for frontend status bar)
    let discovered_model = state
        .discovered_model
        .read()
        .map(|m| m.clone())
        .unwrap_or_default();
    let _ = tx.send(WsOutgoing::new(
        WsMessageType::SessionInfo,
        json!({
            "id": session.id,
            "status": session.status,
            "working_directory": session.working_directory,
            "turn_count": session.turn_count,
            "model": if state.config.model.is_empty() { &discovered_model } else { &state.config.model },
            "permission_mode": session.permission_mode,
        }),
        Some(session_id.clone()),
        next_seq(&seq),
    ));

    info!(session_id = %session_id, "WebSocket connected");

    // Reader loop: process incoming messages
    use futures_util::StreamExt;
    while let Some(Ok(msg)) = ws_rx.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        let incoming: WsIncoming = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = tx.send(WsOutgoing::error(
                    &format!("invalid message: {e}"),
                    Some(session_id.clone()),
                    next_seq(&seq),
                ));
                continue;
            }
        };

        match incoming.msg_type {
            WsMessageType::UserMessage => {
                let content = incoming.data["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();

                if content.is_empty() {
                    let _ = tx.send(WsOutgoing::error(
                        "empty message",
                        Some(session_id.clone()),
                        next_seq(&seq),
                    ));
                    continue;
                }

                // Update session status
                if let Some(mut session) = state.sessions.get(&session_id) {
                    session.status = SessionStatus::Active;
                    session.messages.push(json!({"role": "user", "content": content}));
                    state.sessions.put(session);
                }

                // Send status
                let _ = tx.send(WsOutgoing::new(
                    WsMessageType::Status,
                    json!({"status": "running"}),
                    Some(session_id.clone()),
                    next_seq(&seq),
                ));

                // Dispatch to agent bridge
                let bridge_tx = tx.clone();
                let bridge_state = Arc::clone(&state);
                let bridge_session_id = session_id.clone();
                let bridge_seq = Arc::clone(&seq);
                tokio::spawn(async move {
                    crate::agent_bridge::run_query(
                        bridge_state,
                        bridge_session_id,
                        content,
                        bridge_tx,
                        bridge_seq,
                    )
                    .await;
                });
            }

            WsMessageType::Abort => {
                if let Some(mut session) = state.sessions.get(&session_id) {
                    session.status = SessionStatus::Aborted;
                    state.sessions.put(session);
                }
                // Signal the running agent to stop
                crate::agent_bridge::signal_abort(&session_id);
                let _ = tx.send(WsOutgoing::new(
                    WsMessageType::Status,
                    json!({"status": "aborted"}),
                    Some(session_id.clone()),
                    next_seq(&seq),
                ));
            }

            WsMessageType::SessionCreate => {
                let wd = incoming.data["working_directory"]
                    .as_str()
                    .unwrap_or(&state.config.workspace);
                let new_session = state.sessions.create(wd, &state.config.permission_mode);
                let _ = tx.send(WsOutgoing::new(
                    WsMessageType::SessionInfo,
                    json!({
                        "id": new_session.id,
                        "status": new_session.status,
                        "working_directory": new_session.working_directory,
                    }),
                    Some(new_session.id),
                    next_seq(&seq),
                ));
            }

            WsMessageType::SessionFork => {
                let source_id = incoming.data["session_id"]
                    .as_str()
                    .unwrap_or(&session_id);
                let branch_name = incoming.data["branch_name"]
                    .as_str()
                    .map(String::from);

                if let Some(source) = state.sessions.get(source_id) {
                    let forked = crate::session_store::Session {
                        id: uuid::Uuid::new_v4().to_string().replace('-', ""),
                        status: crate::session_store::SessionStatus::Idle,
                        created_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        updated_at: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                        working_directory: source.working_directory.clone(),
                        model_id: source.model_id.clone(),
                        permission_mode: source.permission_mode.clone(),
                        turn_count: source.turn_count,
                        total_tokens_in: source.total_tokens_in,
                        total_tokens_out: source.total_tokens_out,
                        title: branch_name.as_ref().map(|n| format!("Fork: {n}")).or(source.title.clone()),
                        messages: source.messages.clone(),
                    };
                    let forked_id = forked.id.clone();
                    state.sessions.put(forked);
                    info!(source = %source_id, forked = %forked_id, "session forked");
                    let _ = tx.send(WsOutgoing::new(
                        WsMessageType::SessionInfo,
                        json!({
                            "id": forked_id,
                            "status": "idle",
                            "working_directory": source.working_directory,
                            "turn_count": source.turn_count,
                            "forked_from": source_id,
                            "branch_name": branch_name,
                        }),
                        Some(forked_id),
                        next_seq(&seq),
                    ));
                } else {
                    let _ = tx.send(WsOutgoing::error(
                        &format!("session not found: {source_id}"),
                        Some(session_id.clone()),
                        next_seq(&seq),
                    ));
                }
            }

            WsMessageType::PermissionResponse => {
                crate::agent_bridge::resolve_permission(
                    &incoming.data["request_id"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    incoming.data["allow"].as_bool().unwrap_or(false),
                );
            }

            WsMessageType::ModeChange => {
                let new_mode = incoming.data["mode"]
                    .as_str()
                    .unwrap_or("default")
                    .to_string();
                info!(session_id = %session_id, mode = %new_mode, "permission mode changed");

                // Update session's stored permission mode
                if let Some(mut session) = state.sessions.get(&session_id) {
                    session.permission_mode = new_mode.clone();
                    state.sessions.put(session);
                }

                let _ = tx.send(WsOutgoing::new(
                    WsMessageType::Status,
                    json!({"status": "mode_changed", "mode": new_mode}),
                    Some(session_id.clone()),
                    next_seq(&seq),
                ));
            }

            WsMessageType::SessionSwitch => {
                if let Some(target_id) = incoming.data["session_id"].as_str() {
                    if let Some(target_session) = state.sessions.get(target_id) {
                        info!(from = %session_id, to = %target_id, "session switch");
                        let _ = tx.send(WsOutgoing::new(
                            WsMessageType::SessionInfo,
                            json!({
                                "id": target_session.id,
                                "status": target_session.status,
                                "working_directory": target_session.working_directory,
                                "turn_count": target_session.turn_count,
                                "permission_mode": target_session.permission_mode,
                            }),
                            Some(target_id.to_string()),
                            next_seq(&seq),
                        ));
                    } else {
                        let _ = tx.send(WsOutgoing::error(
                            &format!("session not found: {target_id}"),
                            Some(session_id.clone()),
                            next_seq(&seq),
                        ));
                    }
                }
            }

            WsMessageType::ModelChange => {
                if let Some(new_model) = incoming.data["model"].as_str() {
                    info!(session_id = %session_id, model = %new_model, "model changed");
                    // Update the discovered model for the server
                    if let Ok(mut m) = state.discovered_model.write() {
                        *m = new_model.to_string();
                    }
                    let _ = tx.send(WsOutgoing::new(
                        WsMessageType::Status,
                        json!({"status": "model_changed", "model": new_model}),
                        Some(session_id.clone()),
                        next_seq(&seq),
                    ));
                }
            }

            other => {
                warn!(?other, "unhandled WebSocket message type");
                let _ = tx.send(WsOutgoing::error(
                    &format!("unhandled message type: {other:?}"),
                    Some(session_id.clone()),
                    next_seq(&seq),
                ));
            }
        }
    }

    info!(session_id = %session_id, "WebSocket disconnected");
    write_task.abort();
}

fn next_seq(counter: &AtomicU64) -> u64 {
    counter.fetch_add(1, Ordering::Relaxed)
}
