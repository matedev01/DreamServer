# DreamForge WebSocket Protocol

## Connection

```
ws://localhost:3010/ws?token=<api_key>
```

Authentication is via the `token` query parameter. The server validates it using constant-time HMAC comparison. If invalid, the connection is closed with code `4001`.

## Message Format

All messages are JSON objects with this structure:

```json
{
  "type": "<message_type>",
  "data": { ... },
  "session_id": "<optional>",
  "seq": <server-assigned sequence number>
}
```

- `type` (required): One of the `WSMessageType` enum values below
- `data` (optional): Message payload, defaults to `{}`
- `session_id` (optional): Target session ID
- `seq` (server-to-client only): Auto-incrementing sequence number for replay

## Connection Lifecycle

1. Client connects with `?token=<key>`
2. Server sends `session_info` with current session state
3. Client sends messages; server responds with streaming events
4. Server sends `heartbeat` every 30 seconds
5. Client can reconnect and replay from a `seq` value via `session_switch`

---

## Client → Server Messages

### `user_message`

Send a user query to the agent.

```json
{
  "type": "user_message",
  "data": {
    "content": "Read the file src/main.py"
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `data.content` | string | yes | The user's message text |

### `abort`

Cancel a running query.

```json
{ "type": "abort" }
```

### `permission_response`

Respond to a permission request from the agent.

```json
{
  "type": "permission_response",
  "data": {
    "request_id": "abc123",
    "granted": true,
    "remember": true,
    "scope": "session"
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `data.request_id` | string | yes | ID from the `permission_request` |
| `data.granted` | bool | yes | Whether permission is granted |
| `data.remember` | bool | no | Remember for future calls |
| `data.scope` | string | no | `"session"` or `"tool"` |

### `session_create`

Create a new session.

```json
{
  "type": "session_create",
  "data": {
    "working_directory": "/workspace/my-project"
  }
}
```

### `session_switch`

Switch to an existing session.

```json
{
  "type": "session_switch",
  "data": {
    "session_id": "abcdef1234567890abcdef1234567890",
    "last_seq": 42
  }
}
```

If `last_seq` is provided, the server replays all messages after that sequence number.

### `mode_change`

Change the permission mode.

```json
{
  "type": "mode_change",
  "data": {
    "mode": "accept_edits",
    "confirmed": true
  }
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `data.mode` | string | yes | `default`, `plan`, `accept_edits`, `full_auto` |
| `data.confirmed` | bool | for `full_auto` | Required confirmation for full auto mode |

### `model_change`

Switch the LLM model (if supported).

```json
{
  "type": "model_change",
  "data": {
    "model": "qwen3-30b"
  }
}
```

---

## Server → Client Messages

### `session_info`

Session metadata, sent on connect and session switch.

```json
{
  "type": "session_info",
  "data": {
    "session_id": "abcdef...",
    "model": "qwen3-30b",
    "mode": "default",
    "turn_count": 5,
    "context_used_pct": 23.5
  }
}
```

### `assistant_text`

Streaming token from the assistant.

```json
{
  "type": "assistant_text",
  "data": {
    "delta": "Here is",
    "turn_index": 1
  }
}
```

### `assistant_text_done`

End of assistant text for a turn.

```json
{
  "type": "assistant_text_done",
  "data": {
    "full_text": "Here is the complete response...",
    "turn_index": 1
  }
}
```

### `tool_call_start`

Agent is executing a tool.

```json
{
  "type": "tool_call_start",
  "data": {
    "tool_call_id": "tc_123",
    "tool_name": "read_file",
    "arguments": { "file_path": "src/main.py" },
    "access_level": "read",
    "turn_index": 1
  }
}
```

### `tool_call_result`

Tool execution completed.

```json
{
  "type": "tool_call_result",
  "data": {
    "tool_call_id": "tc_123",
    "tool_name": "read_file",
    "content": "1\timport sys...",
    "is_error": false,
    "duration_ms": 12,
    "truncated": false
  }
}
```

### `permission_request`

Agent needs user permission to proceed.

```json
{
  "type": "permission_request",
  "data": {
    "request_id": "perm_abc",
    "tool_name": "bash",
    "description": "Run: npm install",
    "risk_level": "medium",
    "command": "npm install",
    "file_path": null
  }
}
```

### `status`

Agent state changed.

```json
{
  "type": "status",
  "data": {
    "state": "running",
    "turn_index": 2,
    "detail": "Executing tool"
  }
}
```

States: `idle`, `running`, `waiting_permission`, `compacting`

### `turn_complete`

One agent turn finished.

```json
{
  "type": "turn_complete",
  "data": {
    "turn_index": 2,
    "tokens_in": 1500,
    "tokens_out": 350,
    "tool_calls_count": 2,
    "duration_ms": 4200
  }
}
```

### `query_complete`

Entire query finished.

```json
{
  "type": "query_complete",
  "data": {
    "total_turns": 3,
    "total_tokens_in": 4500,
    "total_tokens_out": 1200,
    "terminal_condition": "natural_stop",
    "duration_ms": 12000
  }
}
```

### `token_usage`

Current token usage stats.

```json
{
  "type": "token_usage",
  "data": {
    "session_total_in": 10000,
    "session_total_out": 3000,
    "context_used_pct": 45.2,
    "budget_remaining_tokens": null
  }
}
```

### `compaction_notice`

Context was compacted to free space.

```json
{
  "type": "compaction_notice",
  "data": {
    "stage": "microcompact",
    "tokens_freed": 2000
  }
}
```

Stages: `microcompact`, `full_compact`, `collapse_drain`

### `error`

An error occurred.

```json
{
  "type": "error",
  "data": {
    "code": "parse_error",
    "message": "Invalid JSON",
    "recoverable": true,
    "turn_index": null
  }
}
```

Error codes: `parse_error`, `validation_error`, `message_too_large`, `session_not_found`, `invalid_mode`, `query_error`

### `heartbeat`

Keep-alive ping (every 30 seconds).

```json
{
  "type": "heartbeat",
  "data": {
    "server_time": "2025-01-15T10:30:00Z"
  }
}
```
