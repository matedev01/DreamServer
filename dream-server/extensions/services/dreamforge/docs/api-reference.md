# DreamForge REST API Reference

## Authentication

All REST endpoints require a Bearer token in the `Authorization` header:

```
Authorization: Bearer <api_key>
```

The API key is auto-generated on first run (stored in `/data/dreamforge/api-key.txt`) or can be set via `DREAMFORGE_API_KEY` environment variable.

---

## Health

### `GET /health`

Health check (no auth required).

**Response:**
```json
{
  "status": "ok",
  "service": "dreamforge",
  "timestamp": "2025-01-15T10:30:00Z"
}
```

### `GET /readyz`

Readiness check — verifies LLM model is configured.

**Response:**
```json
{
  "status": "ready",
  "model": "qwen3-30b",
  "timestamp": "2025-01-15T10:30:00Z"
}
```

---

## Sessions

### `GET /api/sessions`

List all sessions.

**Response:**
```json
{
  "sessions": [
    {
      "id": "abcdef1234567890abcdef1234567890",
      "title": "Fix authentication bug",
      "status": "active",
      "turn_count": 12,
      "created_at": "2025-01-15T10:00:00Z",
      "updated_at": "2025-01-15T10:30:00Z",
      "model_id": "qwen3-30b"
    }
  ]
}
```

### `GET /api/sessions/{session_id}`

Get a single session with message history.

**Parameters:**
- `session_id`: 32-character hex UUID (regex: `^[a-f0-9]{32}$`)

**Response:**
```json
{
  "id": "abcdef...",
  "title": "Fix authentication bug",
  "status": "active",
  "messages": [...],
  "turn_count": 12,
  "permission_mode": "default",
  "working_directory": "/workspace",
  "created_at": "...",
  "updated_at": "..."
}
```

### `POST /api/sessions`

Create a new session.

**Request body:**
```json
{
  "working_directory": "/workspace/my-project"
}
```

All fields are optional. Returns the new session object.

### `DELETE /api/sessions/{session_id}`

Delete a session and its persisted data.

**Response:** `204 No Content`

---

## Memory

### `GET /api/memory`

List all memory entries.

**Response:**
```json
{
  "entries": [
    {
      "id": "memory_abc",
      "type": "user",
      "title": "User is a data scientist",
      "description": "Prefers pandas over SQL",
      "created_at": "2025-01-15T10:00:00Z"
    }
  ],
  "count": 1
}
```

### `GET /api/memory/{entry_id}`

Get a single memory with full content.

**Response:**
```json
{
  "id": "memory_abc",
  "type": "user",
  "title": "User is a data scientist",
  "description": "...",
  "content": "Full markdown content...",
  "created_at": "..."
}
```

### `POST /api/memory`

Create a new memory.

**Request body:**
```json
{
  "type": "user",
  "title": "User is a data scientist",
  "content": "Prefers pandas and matplotlib.",
  "description": "User role and tool preferences"
}
```

Valid types: `user`, `feedback`, `project`, `reference`

### `PUT /api/memory/{entry_id}`

Update a memory (partial update).

**Request body:**
```json
{
  "title": "Updated title",
  "content": "Updated content"
}
```

### `DELETE /api/memory/{entry_id}`

Delete a memory entry.

**Response:** `204 No Content`

---

## Usage

### `GET /api/usage`

Global token usage statistics.

**Response:**
```json
{
  "tokens_in": 50000,
  "tokens_out": 15000,
  "total_tokens": 65000,
  "api_calls": 42,
  "sessions": 3
}
```

### `GET /api/usage/{session_id}`

Per-session token usage.

**Response:**
```json
{
  "session_id": "abcdef...",
  "tokens_in": 10000,
  "tokens_out": 3000,
  "total_tokens": 13000,
  "api_calls": 8
}
```

---

## WebSocket

### `WS /ws?token=<api_key>`

Main agent communication channel. See [WebSocket Protocol](websocket-protocol.md) for full message specification.
