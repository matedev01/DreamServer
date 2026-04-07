# Configuration Reference

This document is a comprehensive reference for all configuration options available in Dream-Forge. Configuration is applied through environment variables, configuration files, and internal defaults.

For related topics, see:

- [Permissions Guide](permissions-guide.md) -- permission modes, rules, and organization policies
- [MCP Configuration](mcp-configuration.md) -- MCP server definitions and tool routing
- [Security Model](security-model.md) -- threat model, containment, and audit logging

---

## Configuration Precedence

Dream-Forge resolves configuration values in the following order (highest priority first):

1. **Environment variables** -- always take precedence when set.
2. **Configuration files** -- `mcp-servers.json`, permission YAML files, `org-policy.yaml`.
3. **Built-in defaults** -- hardcoded fallback values defined in the Rust configuration module.

Setting an environment variable will override any value found in a config file or default.

---

## Environment Variables

All environment variables below are read at startup from the configuration module. Restart the server after changing any value.

### Paths

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_DATA_DIR` | `/data/dreamforge` | Root data directory. Stores sessions, memory, and the API key file. |
| `DREAMFORGE_WORKSPACE` | `/workspace` | Working directory for agent commands. All file operations are relative to this path unless containment is relaxed. |

### Server

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_PORT` | `3010` | Port the web server listens on. |
| `DREAMFORGE_PERMISSION_MODE` | `default` | Permission mode that governs which tool calls require user approval. Accepted values: `default`, `plan`, `accept_edits`, `full_auto`. See [Permissions Guide](permissions-guide.md) for details. |
| `DREAMFORGE_MAX_TURNS` | `200` | Maximum query-loop turns the agent may execute in a single conversation before stopping. |

### LLM

| Variable | Default | Description |
|---|---|---|
| `LLM_API_URL` | `http://llama-server:8080` | Base URL of the LLM inference server. |
| `LLM_API_BASE_PATH` | `/v1` | OpenAI-compatible API path appended to `LLM_API_URL` (e.g., the full chat completions endpoint becomes `LLM_API_URL` + `LLM_API_BASE_PATH` + `/chat/completions`). |

### Optional Services

These variables point to companion containers or external services. Features that depend on an unavailable service degrade gracefully.

| Variable | Default | Description |
|---|---|---|
| `TOKEN_SPY_URL` | `http://token-spy:8080` | Token usage tracking service. |
| `APE_URL` | `http://ape:7890` | Policy enforcement engine. See [Security Model](security-model.md). |
| `QDRANT_URL` | `http://qdrant:6333` | Qdrant vector database used for RAG retrieval. |
| `SEARXNG_URL` | `http://searxng:8080` | SearXNG web search backend for the `web_search` tool. |
| `DASHBOARD_API_URL` | `http://dashboard-api:3002` | Dashboard API for GPU metrics and monitoring. |
| `GPU_BACKEND` | `nvidia` | GPU backend used for inference. Accepted values: `nvidia`, `amd`, `apple`, `cpu`. |
| `EMBEDDINGS_URL` | `http://embeddings:80` | Embedding model server used for RAG document indexing. |
| `WHISPER_URL` | `http://whisper:8000` | Whisper speech-to-text server for voice input. |
| `TTS_URL` | `http://tts:8880` | Text-to-speech server for voice output. |
| `DREAMFORGE_TTS_VOICE` | `af_heart` | Voice name passed to the TTS server. |

### RAG (Retrieval-Augmented Generation)

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_RAG_CHUNK_SIZE` | `512` | Number of characters per chunk when splitting documents for indexing. |
| `DREAMFORGE_RAG_CHUNK_OVERLAP` | `64` | Overlap in characters between consecutive chunks. Prevents information loss at chunk boundaries. |
| `DREAMFORGE_RAG_COLLECTION` | `default` | Qdrant collection name used to store and query document embeddings. |
| `DREAMFORGE_MAX_UPLOAD_SIZE` | `52428800` (50 MB) | Maximum file upload size in bytes. |

### Security

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_API_KEY` | *(auto-generated)* | API key used for authentication. See [API Key Management](#api-key-management) below. |
| `DREAMFORGE_APE_FAIL_MODE` | `closed` | Behavior when the APE policy engine is unreachable. `closed` denies the action; `warn_open` logs a warning and allows it. See [Security Model](security-model.md). |
| `DREAMFORGE_READ_CONTAINMENT` | `system` | Read scope for file operations. `system` allows reading anywhere on the filesystem; `workspace` restricts reads to `DREAMFORGE_WORKSPACE`. |
| `DREAMFORGE_MAX_BASH_TIMEOUT` | `600` | Maximum allowed timeout for a single bash command, in seconds. |
| `DREAMFORGE_PERMISSION_TIMEOUT` | `60` | Seconds to wait for a user response on a permission prompt before the action is denied. |
| `DREAMFORGE_AUDIT_LOG` | `true` | Enable structured audit logging of all tool calls and permission decisions. |
| `DREAMFORGE_SECRET_SCANNING` | `true` | Redact detected credentials and secrets from tool output before it reaches the LLM or the client. |

### Rate Limiting

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_RATE_LIMIT` | `60` | Maximum tool calls per minute per session. Applies globally across all tools. |
| `DREAMFORGE_TOOL_RATE_OVERRIDES` | `{}` | JSON object mapping tool names to per-tool rate limits. Example: `'{"bash": 20, "write_file": 10}'`. Overrides the global limit for the specified tools only. |

### Token Budget

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_TOKEN_BUDGET` | `0` | Maximum tokens the agent may consume in a session. `0` means unlimited. Requires `TOKEN_SPY_URL` to be reachable for enforcement. |

### MCP (Model Context Protocol)

| Variable | Default | Description |
|---|---|---|
| `DREAMFORGE_MCP_TOOL_TIMEOUT` | `30` | Default timeout in seconds for MCP tool calls. Cannot exceed the hard cap of 120 seconds (see internal constants). See [MCP Configuration](mcp-configuration.md). |

---

## Internal Constants

The following values are defined in the configuration module and cannot be changed via environment variables. They are documented here for reference and troubleshooting.

| Constant | Value | Description |
|---|---|---|
| `TOOL_RESULT_MAX_CHARS` | 50,000 | Maximum characters in a tool result before it is truncated. |
| `HEARTBEAT_INTERVAL` | 30 s | WebSocket heartbeat interval. |
| `SESSION_AUTOSAVE_INTERVAL` | 10 s | How often session state is written to disk. |
| `MAX_WS_MESSAGE_SIZE` | 1,000,000 (1 MB) | Maximum WebSocket message size. Messages exceeding this are rejected. |
| `MAX_USER_MESSAGE_CHARS` | 500,000 | Maximum character length for a single user message. |
| `MCP_MAX_TOOL_TIMEOUT` | 120 s | Hard upper bound on `DREAMFORGE_MCP_TOOL_TIMEOUT`. |
| `MAX_FILE_TREE_ENTRIES` | 30 | Maximum files shown in the workspace file tree widget. |
| `MAX_SESSION_MESSAGES_STORED` | 200 | Maximum messages retained in a single session before older messages are evicted. |
| `WS_REPLAY_BUFFER_SIZE` | 500 | Number of WebSocket messages kept in the replay buffer for reconnecting clients. |
| `TOOL_RESULT_WS_PREVIEW_CHARS` | 2,000 | Character limit for tool result previews sent over the WebSocket to the UI. |
| `MAX_READ_FILE_LINES` | 10,000 | Maximum lines the `read_file` tool will return in one call. |
| `DEFAULT_READ_FILE_LINES` | 2,000 | Default line count for `read_file` when no limit is specified. |

---

## Configuration Files

In addition to environment variables, Dream-Forge reads the following files at startup or on demand:

| File | Purpose |
|---|---|
| `.forge/settings.json` | Defines available MCP servers, their transport, and tool routing. See [MCP Configuration](mcp-configuration.md). |
| Permission rule YAML files | Customize which tool calls require approval and under what conditions. See [Permissions Guide](permissions-guide.md). |
| `org-policy.yaml` | Organization-wide policy overrides applied before user-level permissions. See [Permissions Guide](permissions-guide.md). |

---

## API Key Management

Dream-Forge requires an API key for authenticating client connections. The key is resolved using the following priority:

1. **`DREAMFORGE_API_KEY` environment variable** -- if set, this value is used directly.
2. **`{DREAMFORGE_DATA_DIR}/api-key.txt` file** -- if the env var is not set, the key is read from this file.
3. **Auto-generation** -- if neither the env var nor the file exists, a random key is generated at startup and written to `api-key.txt`.

On Unix systems, auto-generated key files are created with `0o600` permissions (owner read/write only).

On Windows, file-level permission enforcement is not available. It is strongly recommended to set the `DREAMFORGE_API_KEY` environment variable directly rather than relying on the auto-generated file.

---

## Quick Start Example

A minimal `docker-compose.override.yml` snippet customizing common settings:

```yaml
services:
  dreamforge:
    environment:
      DREAMFORGE_PORT: "3010"
      DREAMFORGE_PERMISSION_MODE: "accept_edits"
      DREAMFORGE_WORKSPACE: "/workspace"
      DREAMFORGE_API_KEY: "your-secret-key-here"
      DREAMFORGE_RAG_CHUNK_SIZE: "1024"
      DREAMFORGE_RATE_LIMIT: "120"
      GPU_BACKEND: "nvidia"
```
