# DreamForge

A local agentic coding tool that runs entirely on your own GPU. No cloud dependency. No data leaves your machine.

DreamForge connects to a local LLM (via [LM Studio](https://lmstudio.ai), [llama.cpp](https://github.com/ggml-org/llama.cpp), [vLLM](https://github.com/vllm-project/vllm), or [Ollama](https://ollama.com)) and provides a web-based interface for AI-assisted coding — reading files, writing code, running commands, searching your codebase, and iterating autonomously. Built as an extension for [DreamServer](https://github.com/Light-Heart-Labs/DreamServer).

> **Status: Production-ready.** 30+ active tools (including 5 DreamServer-native tools), local-first conversation runtime with loop detection and JSON recovery, MCP protocol, RAG pipeline, memory system, model-adaptive prompts, configurable permissions, session persistence, and full DreamServer integration. Tested end-to-end with Qwen3.5-27B.

---

## Quickstart

### Prerequisites

- **Rust 1.86+** (for local dev) OR **Docker** (for deployment)
- A running LLM with an OpenAI-compatible API (LM Studio, llama-server, vLLM, Ollama)
- **30B+ parameter model recommended** — smaller models (< 7B) can't reliably produce tool calls

### Run locally (Rust engine)

```bash
# Clone
git clone https://github.com/Light-Heart-Labs/Dream-Forge.git
cd Dream-Forge/rust

# Start (assumes LM Studio on port 1234)
DREAMFORGE_MODEL=qwen3-30b-a3b-instruct \
LLM_API_URL=http://localhost:1234/v1 \
DREAMFORGE_PORT=3010 \
DREAMFORGE_WORKSPACE=../workspace \
cargo run -p dreamforge-server
```

Open `http://localhost:3010` in your browser.

> **Note:** `LLM_API_URL` must include `/v1` for OpenAI-compatible endpoints. The Rust engine appends `/chat/completions` directly.

To point at a different LLM server:

```bash
LLM_API_URL=http://localhost:8080/v1 cargo run -p dreamforge-server   # llama-server
LLM_API_URL=http://localhost:11434/v1 cargo run -p dreamforge-server  # Ollama
LLM_API_URL=http://localhost:8000/v1 cargo run -p dreamforge-server   # vLLM
```

### Run via Docker (as DreamServer extension)

```bash
docker compose -f compose.rust.yaml up -d dreamforge
```

This builds the Rust binary, bundles the frontend, and runs on `dream-network` alongside other DreamServer services. Includes Qdrant and TEI embeddings for RAG.

To use LM Studio from inside Docker instead of llama-server:

```bash
LLM_API_URL=http://host.docker.internal:1234/v1 docker compose -f compose.rust.yaml up -d dreamforge
```

See the [Docker Guide](docs/docker-guide.md) for full Docker Compose setup.

---

## Architecture

```
User (browser) <--WebSocket--> Axum Server <--OpenAI-compat--> LLM (local GPU)
                                   |
                     +-------------+-------------+
                     |             |             |
              ConversationRuntime  MCP Servers   RAG Pipeline
                     |             |             |
               Tool dispatch    External     Qdrant + TEI
               via execute_tool  tool servers  embeddings
```

### Rust backend (12 source files, ~3,100 lines)

| File | Lines | Purpose |
|------|-------|---------|
| `agent_bridge.rs` | 917 | Core agent loop, tool dispatch, streaming, abort |
| `routes.rs` | 355 | REST API endpoints (health, bootstrap, sessions, models) |
| `ws.rs` | 338 | WebSocket handler, session management, abort signaling |
| `files.rs` | 264 | File read/save/tree routes with workspace path validation |
| `lib.rs` | 284 | Server startup, MCP init, graceful shutdown |
| `session_store.rs` | 260 | Session CRUD + disk persistence (JSON) |
| `documents.rs` | 147 | RAG document upload/search/collections routes |
| `memory_routes.rs` | 143 | Memory CRUD routes |
| `ws_types.rs` | 131 | WebSocket protocol message types |
| `voice.rs` | 126 | STT (Whisper) / TTS (Kokoro) proxy |
| `config.rs` | 124 | Environment variable configuration |
| `main.rs` | 28 | Entry point |

The server wraps DreamForge's `LocalConversationRuntime` (with integrated loop detection, token budget tracking, malformed JSON recovery, and model tier awareness) and `execute_tool()` dispatcher, adding HTTP/WebSocket transport, MCP server management, RAG pipeline integration, and session persistence.

### Frontend (23 React components)

| Component | Purpose |
|-----------|---------|
| `ChatPanel` | Streaming message display, auto-scroll, input bar with send/abort |
| `MessageBubble` | Markdown rendering with syntax highlighting and copy button |
| `ToolCallCard` | Expandable tool execution cards with diff view for edits |
| `PermissionDialog` | Risk-colored permission prompt with remember/scope options |
| `StatusBar` | Model name, connection status, token usage bar, agent state |
| `SessionSidebar` | Session list with create, switch, delete |
| `MemoryPanel` | Browse, create, edit, delete persistent memories |
| `SettingsPage` | Model info, permission mode, memory stats, MCP config |
| `OnboardingWizard` | First-run walkthrough |
| `ModeSwitch` | Segmented control for permission modes |
| `ForgeContext` | WebSocket state management, reconnection, message dispatch |
| `CodeEditor` | Integrated code editor panel |
| `CodeBlock` | Syntax-highlighted code blocks with copy button |
| `CommandPalette` | Keyboard-driven command palette |
| `FileTreeBrowser` | Workspace file tree navigator |
| `DocumentPanel` | Document viewer side panel |
| `EditorPanel` | Editor panel container |
| `DiffViewer` | Unified diff display for file edits |
| `StreamingMarkdown` | Streaming markdown renderer |
| `ToolCallVisualization` | Tool call visual indicator |
| `VirtualMessageList` | Virtualized message list for performance |
| `VoiceButton` | Voice input recording |
| `TTSButton` | Text-to-speech playback |

---

## How it works

1. **You type a message** in the browser. It goes over WebSocket to the Axum server.
2. **The agent loop** builds a rich system prompt (git status, file tree, memory, CLAUDE.md instructions, OS info), then calls the local LLM via the OpenAI-compatible streaming API.
3. **When the LLM calls a tool** (e.g., "read this file", "run this command"), the call is dispatched through `execute_tool()` — 23+ tools available including bash, file I/O, search, web fetch, task management, and more.
4. **Tool results go back to the LLM** for the next iteration. The loop continues until the LLM responds without calling tools (task complete), hits a terminal condition (error, abort, max turns), or you press Escape.
5. **Context compaction** kicks in when token count exceeds the threshold — the conversation is summarized to free context space while preserving key information.
6. **Sessions are persisted** to disk as JSON, surviving server restarts. Memories are stored separately and injected into future system prompts.

---

## What works

### Core Agent
- Streaming LLM responses via OpenAI-compatible API
- 30+ active tools via `execute_tool()` dispatcher including `bash`, `read_file`, `write_file`, `edit_file`, `glob_search`, `grep_search`, `WebFetch`, `WebSearch`, `TodoWrite`, `REPL`, `PowerShell`, `Config`, `RemoteTrigger`, plus 5 DreamServer-native tools: `GenerateImage` (ComfyUI), `RAGSearch` (Qdrant), `ServiceHealth` (Dashboard API), `TextToSpeech` (Kokoro), `SpeechToText` (Whisper)
- Rich system prompt with git status, file tree, CLAUDE.md instructions, OS info
- Context compaction for long conversations
- Graceful abort with cancellation signaling
- Crash recovery on restart

### Permissions & Security
- Configurable permission modes: `full_auto`, `default` (prompt), read-only
- Runtime mode switching via WebSocket
- Secret scanning on tool output (API keys, tokens, credentials)
- Pre/post tool use hooks via `RuntimeFeatureConfig`

### Persistence
- Session persistence to disk (JSON, survives restart)
- Memory system (CRUD + injection into system prompt)
- Session forking for conversation branching

### Integrations
- MCP protocol — stdio server discovery, tool routing, lifecycle management
- RAG pipeline — document upload, semantic search via Qdrant + TEI embeddings
- Voice — STT via Whisper, TTS via Kokoro
- Model auto-detection from LLM endpoint
- Prompt caching (Anthropic provider)
- Retry policies with exponential backoff

### REST API (26 endpoints)

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/health` | Health check |
| `GET` | `/readyz` | Readiness probe |
| `GET` | `/api/bootstrap` | Server config (model, workspace, permissions) |
| `GET` | `/api/sessions` | List sessions |
| `POST` | `/api/sessions` | Create session |
| `GET` | `/api/sessions/{id}` | Get session |
| `DELETE` | `/api/sessions/{id}` | Delete session |
| `POST` | `/api/sessions/{id}/fork` | Fork session |
| `GET` | `/api/sessions/{id}/usage` | Session token usage + cost estimate |
| `GET` | `/api/models` | List available models (proxied from LLM) |
| `GET` | `/api/models/current` | Current model + permission mode |
| `GET` | `/api/files/read` | Read file (workspace-scoped) |
| `POST` | `/api/files/save` | Save file (workspace-scoped) |
| `GET` | `/api/files/tree` | File tree listing |
| `GET` | `/api/memory` | List memories |
| `POST` | `/api/memory` | Create memory |
| `GET` | `/api/memory/{id}` | Get memory |
| `PUT` | `/api/memory/{id}` | Update memory |
| `DELETE` | `/api/memory/{id}` | Delete memory |
| `POST` | `/api/documents/upload` | Upload document for RAG |
| `POST` | `/api/documents/search` | Semantic search |
| `GET` | `/api/documents/collections` | List collections (via Qdrant) |
| `DELETE` | `/api/documents/collections/{name}/documents/{id}` | Delete document |
| `POST` | `/api/voice/transcribe` | Speech-to-text (via Whisper) |
| `POST` | `/api/voice/speak` | Text-to-speech (via Kokoro) |
| `GET` | `/ws` | WebSocket — streaming chat, tool calls, abort |

---

## What's next

1. **Dogfood** — Use it on real coding projects daily, fix what surfaces
2. **Rewrite core conversation loop** — Replace wrapped `ConversationRuntime` with a fully DreamForge-native async runtime
3. **Agent/sub-agent support** — Server-side worker orchestration for parallel task execution
4. **Multi-model routing** — Use LiteLLM to route different tasks to different models
5. **Fine-tuning pipeline** — Learn from user's coding patterns on local GPU

## Engine architecture

DreamForge is built around a local-first `LocalConversationRuntime` that wraps the core conversation loop with features optimized for local model inference:

- **Integrated loop detection** — configurable per-tool call limits with automatic turn abort (not bolted onto the tool executor)
- **Token budget tracking** — proactive compaction before context window exhaustion
- **Model tier awareness** — auto-detects model capability (Tier A >30B / Tier B 7-30B / Tier C <7B) and adapts system prompt complexity
- **Malformed JSON recovery** — strips markdown fences, fixes trailing commas, quotes bare keys (common with smaller local models)
- **DreamServer-native tools** — 5 tools that integrate directly with DreamServer's service mesh (ComfyUI, Qdrant, Dashboard API, Whisper, Kokoro TTS)

Key crates live under `rust/crates/`. The server layer (`dreamforge-server/`), RAG pipeline (`dreamforge-rag/`), local conversation runtime, memory system, RBAC, and frontend are DreamForge-original code.

---

## Configuration

Copy `rust/.env.example` to `rust/.env` and customize. All settings are via environment variables.

### Server

| Variable | Default | Description |
|----------|---------|-------------|
| `DREAMFORGE_HOST` | `0.0.0.0` | Bind address |
| `DREAMFORGE_PORT` | `3010` | Web server port |
| `DREAMFORGE_WORKSPACE` | `.` | Working directory for agent commands |
| `DREAMFORGE_DATA_DIR` | `/data/dreamforge` | Root data directory (sessions, memory) |
| `DREAMFORGE_MAX_TURNS` | `200` | Max agent loop turns per conversation |
| `DREAMFORGE_PERMISSION_MODE` | `default` | Permission mode: `default`, `full_auto`, `read-only` |
| `DREAMFORGE_API_KEY` | (empty = open) | API key for authentication |

### LLM

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_API_URL` | `http://localhost:11434` | LLM server URL. **Must include `/v1`** for OpenAI-compat endpoints |
| `DREAMFORGE_MODEL` | (auto-detect) | Model name. If empty, queries LLM `/models` endpoint |
| `DREAMFORGE_COMPACT_THRESHOLD` | `10000` | Token count before context compaction |
| `DREAMFORGE_COMPACT_PRESERVE` | `4` | Number of recent messages to preserve during compaction |
| `DREAMFORGE_OUTPUT_STYLE` | (empty) | Custom output style directive for system prompt |

### RAG (optional — requires Qdrant + TEI)

| Variable | Default | Description |
|----------|---------|-------------|
| `DREAMFORGE_RAG_ENABLED` | `false` | Enable RAG pipeline |
| `QDRANT_URL` | `http://localhost:6333` | Qdrant vector DB URL |
| `EMBEDDINGS_URL` | `http://localhost:8090` | TEI embeddings service URL |

### Voice (optional — requires Whisper + TTS)

| Variable | Default | Description |
|----------|---------|-------------|
| `WHISPER_URL` | `http://localhost:8000` | Whisper STT service URL |
| `TTS_URL` | `http://localhost:8880` | Kokoro TTS service URL |

### Web Search (optional)

| Variable | Default | Description |
|----------|---------|-------------|
| `SEARXNG_URL` | `http://searxng:8080` | SearXNG search engine URL |

### DreamServer-native tools (optional)

| Variable | Default | Description |
|----------|---------|-------------|
| `COMFYUI_URL` | `http://comfyui:8188` | ComfyUI for image generation |
| `DASHBOARD_API_URL` | `http://dashboard-api:3002` | DreamServer dashboard for service health |

Tools auto-hide from the model when their backing service URL is not configured.

---

## Tool calling tiers

Not all local models handle tool calling the same way:

| Tier | Models | How tools work |
|------|--------|---------------|
| **A** (90%+) | Qwen 2.5/3, Llama 3.1+ | Native `tool_calls` via OpenAI API format |
| **B** (70-85%) | Mistral, DeepSeek | Native format with JSON validation + recovery |
| **C** (<60%) | Phi, Gemma, small models | Tool schemas embedded in system prompt, JSON extracted from text output |

DreamForge auto-detects the tier from the model name and adapts. Tier C models get periodic instruction reinforcement and 3 extraction strategies (tag-based, bare JSON, multi-line).

---

## DreamServer integration

DreamForge is built as a DreamServer extension. When installed in the DreamServer ecosystem, it automatically connects to:

| Service | Port | Used for |
|---------|------|----------|
| llama-server | 8080 | LLM inference (required) |
| LM Studio | 1234 | LLM inference (alternative) |
| Qdrant | 6333 | Vector search for RAG (optional) |
| TEI Embeddings | 80 | Embedding model for RAG (optional) |
| SearXNG | 8888 | Web search tool (optional) |
| Whisper | 8000 | Speech-to-text (optional) |
| TTS (Kokoro) | 8880 | Text-to-speech (optional) |
| Dashboard API | 3002 | GPU metrics (optional) |

Only an LLM server is required. Everything else degrades gracefully.

---

## Project structure

```
Dream-Forge/
  compose.yaml                       # DreamServer extension compose
  compose.rust.yaml                  # Standalone Docker Compose + Qdrant + TEI
  Dockerfile.rust                    # Multi-stage Docker build
  manifest.yaml                      # DreamServer extension manifest
  docs/                              # Documentation (13 guides)
  rust/                              # Rust engine
    Cargo.toml                       # Workspace root
    crates/
      dreamforge-server/             # Axum HTTP/WS server (DreamForge-original)
        agent_bridge.rs              #   LocalConversationRuntime bridge
        ws.rs                        #   WebSocket handler
        routes.rs                    #   REST API endpoints
        session_store.rs             #   Session persistence
        ...                          #   12 files total
      dreamforge-rag/                # RAG pipeline (DreamForge-original)
      dreamforge-cli/                # CLI binary (forge)
      runtime/                       # Conversation runtime
        local_conversation.rs        #   LocalConversationRuntime (DreamForge-original)
        dreamforge_config.rs         #   DreamForge configuration
        memory/                      #   Persistent memory system
        rbac/                        #   Role-based access control
        secret_scanner.rs            #   Credential detection
        ...                          #   48 files total
      api/                           # LLM provider clients (OpenAI-compat, Anthropic optional)
      tools/                         # 30+ tool definitions and execution
      commands/                      # CLI commands (agents, skills, MCP)
      plugins/                       # Plugin lifecycle management
      telemetry/                     # Client identity and request profiling
    frontend/                        # React frontend (Vite + Tailwind)
      src/components/
        ChatPanel.jsx                #   Message display + input
        ToolCallCard.jsx             #   Rich tool output rendering
        ActivityIndicator.jsx        #   Animated progress with elapsed time
        ServicePanel.jsx             #   DreamServer health dashboard
        SessionSidebar.jsx           #   Session history + search + fork
        ...
```

---

## Documentation

### Getting Started
- [Getting Started Guide](docs/getting-started.md) — Install, configure, and run DreamForge
- [Docker Guide](docs/docker-guide.md) — Docker Compose setup and configuration
- [Troubleshooting](docs/troubleshooting.md) — Common issues and fixes

### User Guides
- [Permissions Guide](docs/permissions-guide.md) — Permission modes, prompts, grants, and rules
- [Security Model](docs/security-model.md) — What's blocked, what's allowed, and why
- [Memory Guide](docs/memory-guide.md) — Persistent memory across sessions
- [MCP Configuration](docs/mcp-configuration.md) — Extend the agent with external MCP servers

### Developer Guides
- [Tool Development Guide](docs/tool-development.md) — Creating new tools
- [Frontend Development Guide](docs/frontend-development.md) — React component architecture
- [Testing Guide](docs/testing-guide.md) — Running and writing tests

### Reference
- [Configuration Reference](docs/configuration-reference.md) — All environment variables
- [API Reference](docs/api-reference.md) — REST endpoint documentation
- [WebSocket Protocol](docs/websocket-protocol.md) — Message type specification
---

## License

See [LICENSE](LICENSE).

---

Built by [Light Heart Labs](https://github.com/Light-Heart-Labs). Architecture derived from the [DreamServer agent-systems blueprint](https://github.com/Light-Heart-Labs/DreamServer/tree/main/resources/research/agent-systems).
