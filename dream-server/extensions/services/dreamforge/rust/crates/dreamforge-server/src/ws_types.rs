//! WebSocket message types for the DreamForge protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------- message type enum ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WsMessageType {
    // Client → Server
    UserMessage,
    Abort,
    PermissionResponse,
    SessionCreate,
    SessionSwitch,
    SessionResume,
    SessionFork,
    ModeChange,
    ModelChange,
    UserInputResponse,

    // Server → Client
    AssistantText,
    AssistantTextDone,
    ToolCallStart,
    ToolCallResult,
    PermissionRequest,
    Error,
    SessionInfo,
    Status,
    TurnComplete,
    QueryComplete,
    CompactionNotice,
    TokenUsage,
    Heartbeat,
    UserInputRequest,
    TodoUpdate,
}

// ---------- incoming (client → server) ----------

#[derive(Debug, Clone, Deserialize)]
pub struct WsIncoming {
    #[serde(rename = "type")]
    pub msg_type: WsMessageType,
    #[serde(default)]
    pub data: Value,
    pub session_id: Option<String>,
}

// ---------- outgoing (server → client) ----------

#[derive(Debug, Clone, Serialize)]
pub struct WsOutgoing {
    #[serde(rename = "type")]
    pub msg_type: WsMessageType,
    pub data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub timestamp: String,
    pub seq: u64,
}

impl WsOutgoing {
    /// Create a new outgoing message with auto-populated timestamp.
    pub fn new(msg_type: WsMessageType, data: Value, session_id: Option<String>, seq: u64) -> Self {
        Self {
            msg_type,
            data,
            session_id,
            timestamp: chrono_now(),
            seq,
        }
    }

    /// Convenience: create an error message.
    pub fn error(message: &str, session_id: Option<String>, seq: u64) -> Self {
        Self::new(
            WsMessageType::Error,
            serde_json::json!({ "message": message }),
            session_id,
            seq,
        )
    }
}

/// Returns an ISO 8601 timestamp string using `std::time`.
fn chrono_now() -> String {
    // Use a simple approach without pulling in the chrono crate.
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Simple ISO-ish format: seconds since epoch (frontend can parse either)
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_deserializes_user_message() {
        let json = r#"{"type":"user_message","data":{"content":"hello"}}"#;
        let msg: WsIncoming = serde_json::from_str(json).unwrap();
        assert_eq!(msg.msg_type, WsMessageType::UserMessage);
        assert_eq!(msg.data["content"], "hello");
    }

    #[test]
    fn outgoing_serializes_correctly() {
        let msg = WsOutgoing::new(
            WsMessageType::AssistantText,
            serde_json::json!({"delta": "hi"}),
            Some("sess1".to_string()),
            1,
        );
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("assistant_text"));
        assert!(json.contains("sess1"));
    }

    #[test]
    fn error_message_includes_message_field() {
        let msg = WsOutgoing::error("something broke", None, 0);
        assert_eq!(msg.msg_type, WsMessageType::Error);
        assert_eq!(msg.data["message"], "something broke");
    }
}
