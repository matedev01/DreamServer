# Getting Started

This guide walks you through setting up DreamForge from scratch and sending your first message.

---

## Prerequisites

- **Node.js 18+** — [nodejs.org](https://nodejs.org/)
- **A running LLM server** with an OpenAI-compatible API (see below)

## Step 1: Choose and Start an LLM Server

DreamForge connects to a local LLM server. You need one running before you start. Pick whichever you prefer:

### llama.cpp (llama-server)

Best for: direct GPU control, lowest overhead.

```bash
# Download a model (example: Qwen 3 30B)
# Then start the server:
llama-server -m qwen3-30b-q4_k_m.gguf -c 32768 --port 8080
```

DreamForge default expects `http://localhost:8080`.

### Ollama

Best for: easy model management, one-command setup.

```bash
ollama pull qwen3:30b
ollama serve   # runs on port 11434
```

Set `LLM_API_URL=http://localhost:11434` when starting DreamForge.

### vLLM

Best for: high-throughput, multi-GPU setups.

```bash
vllm serve qwen3-30b --port 8000
```

Set `LLM_API_URL=http://localhost:8000` when starting DreamForge.

### LM Studio

Best for: GUI-based model management on desktop.

1. Download and install LM Studio
2. Load a model and start the local server (usually port 1234)
3. Set `LLM_API_URL=http://localhost:1234` when starting DreamForge

### Which Model to Use?

DreamForge works best with models that support native tool calling:

| Tier | Models | Tool Calling |
|------|--------|-------------|
| A (best) | Qwen 2.5/3, Llama 3.1+ | Native tool_calls via OpenAI API |
| B (good) | Mistral, DeepSeek | Native with JSON validation + recovery |
| C (basic) | Phi, Gemma, small models | Tool schemas in system prompt, text extraction |

DreamForge auto-detects the tier from the model name and adapts. For the best experience, use a Tier A model with at least 14B parameters.

## Step 2: Install DreamForge

```bash
# Clone the repository
git clone https://github.com/Light-Heart-Labs/Dream-Forge.git
cd Dream-Forge

# Install frontend dependencies
cd rust/frontend
npm install
cd ../..
```

## Step 3: Configure (Optional)

Copy the example environment file and customize if needed:

```bash
cp .env.example .env
# Edit .env to change defaults
```

Most defaults work out of the box. The main thing you might need to change is `LLM_API_URL` if your LLM server isn't on port 8080.

See [Configuration Reference](configuration-reference.md) for all options.

## Step 4: Start DreamForge

```bash
# Start both backend and frontend
./dev.sh

# Or point at a different LLM server:
LLM_API_URL=http://localhost:11434 ./dev.sh   # Ollama
LLM_API_URL=http://localhost:8000 ./dev.sh    # vLLM
```

You can also start backend and frontend separately:

```bash
./dev.sh backend    # backend only (port 3011)
./dev.sh frontend   # frontend only (port 3010, proxies to backend)
```

## Step 5: Open the UI

Open **http://localhost:3010** in your browser.

On first launch, you'll see:
1. **API key generation** — an API key is auto-generated and saved to `data/dreamforge/api-key.txt`. The frontend reads it automatically.
2. **Onboarding wizard** — a brief walkthrough of the interface.

## Step 6: Send Your First Message

Type a message in the input bar at the bottom and press Enter. Try something like:

- "Read the file README.md and summarize it"
- "List all Python files in this directory"
- "What's in the current git branch?"

### What Happens When You Send a Message

1. Your message goes over WebSocket to the Rust backend
2. The **query loop** builds a system prompt (with memory, file tree, git branch), calls your local LLM, and streams tokens back to your browser
3. When the LLM calls a tool (e.g., "read this file"), the call passes through the **7-step security pipeline**: schema validation, shell security parsing, permission check, APE policy check, execution, output truncation, and audit logging
4. If a tool needs permission (e.g., bash commands, file writes), you'll see a **permission dialog** — approve or deny
5. Tool results go back to the LLM for the next iteration
6. The loop continues until the LLM responds without calling tools (task complete) or you press **Escape** to abort

## The Interface

### Main Panels

- **Chat Panel** (center) — streaming message display with tool execution cards
- **Session Sidebar** (left) — switch between conversations, create new sessions
- **Status Bar** (top) — model name, connection status, token usage, agent state

### Side Panels (toggle with buttons or keyboard)

- **Memory Panel** (Ctrl+Shift+M) — browse, create, edit, delete persistent memories
- **Document Panel** (Ctrl+Shift+D) — document viewer
- **Code Editor** (Ctrl+Shift+E) — integrated code editor
- **Settings** (Ctrl+/) — model info, permission mode, memory stats, MCP config

### Permission Modes

The mode selector in Settings controls how much autonomy the agent has:

| Mode | What It Means |
|------|---------------|
| Default | Agent can read freely but asks before writing files or running commands |
| Plan | Read-only — agent can explore but can't change anything |
| Accept Edits | Agent can write files without asking, but still asks for commands |
| Full Auto | Agent runs autonomously (use with caution) |

See [Permissions Guide](permissions-guide.md) for details.

## Common First-Run Issues

### "Connection failed" or WebSocket won't connect

- Make sure both backend and frontend are running (`./dev.sh` starts both)
- Check that port 3010 (frontend) and 3011 (backend) are free
- Check browser console for WebSocket errors

### "Model not found" or empty responses

- Verify your LLM server is running and accessible
- Check the URL: `curl http://localhost:8080/v1/models` should return model info
- Make sure `LLM_API_URL` matches your server's actual address and port

### Permission prompts for everything

- This is normal in `default` mode — the agent asks before running commands or writing files
- Switch to `accept_edits` mode if you trust the agent with file changes
- See [Permissions Guide](permissions-guide.md) for mode details

### Slow responses

- Check your LLM server's GPU utilization
- Larger models (30B+) need significant GPU memory
- Consider a smaller model (14B) for faster iteration
- Context window size affects speed — smaller contexts are faster

---

## Next Steps

- [Permissions Guide](permissions-guide.md) — understand permission modes and prompts
- [Security Model](security-model.md) — what the agent can and can't do
- [Memory Guide](memory-guide.md) — persistent context across sessions
- [MCP Configuration](mcp-configuration.md) — extend the agent with external tools
- [Configuration Reference](configuration-reference.md) — all environment variables
- [Docker Guide](docker-guide.md) — run with Docker/Docker Compose
