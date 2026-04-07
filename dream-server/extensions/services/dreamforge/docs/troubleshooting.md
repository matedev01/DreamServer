# Troubleshooting

Common issues and how to fix them.

---

## Installation

### Node.js version error

```
error engine "node": Incompatible
```

DreamForge requires Node.js 18+. Check with `node --version`. Install from [nodejs.org](https://nodejs.org/) or use nvm:

```bash
nvm install 18
nvm use 18
```

---

## LLM Connection

### "Connection refused" or "Cannot connect to LLM server"

1. Verify your LLM server is running:
   ```bash
   curl http://localhost:8080/v1/models
   ```
2. Check the URL matches what DreamForge expects:
   - Default: `http://localhost:8080` (llama-server)
   - Ollama: `http://localhost:11434`
   - vLLM: `http://localhost:8000`
3. Set the correct URL:
   ```bash
   LLM_API_URL=http://localhost:11434 ./dev.sh
   ```

### "Model not found" or empty responses

- The LLM server is running but no model is loaded
- For llama-server: make sure you passed a model file (`-m model.gguf`)
- For Ollama: `ollama pull qwen3:30b` before starting
- Test: `curl http://localhost:8080/v1/models` should list available models

### Responses are very slow

- Check GPU utilization (`nvidia-smi` for NVIDIA)
- Model may be too large for your GPU — try a smaller model
- Context window size affects speed. DreamForge adjusts automatically based on the model, but very long conversations get slower
- Check if the model is running on CPU instead of GPU

### "Context window exceeded" or "Prompt too long"

- DreamForge has automatic context compaction that kicks in when approaching the limit
- If you see this error, the conversation has grown very large
- Start a new session to reset the context
- Compaction stages: microcompact (drop old tool results) -> full compact (LLM summarization) -> collapse drain (emergency)

### Model tier detection is wrong

DreamForge auto-detects the model tier (A/B/C) from the model name. If detection is wrong:
- Tier A models: Qwen 2.5/3, Llama 3.1+ (native tool calling)
- Tier B models: Mistral, DeepSeek (native with recovery)
- Tier C models: Phi, Gemma, small models (text-based tool calling)
- Check the StatusBar in the UI for the detected model name and tier

---

## WebSocket Connection

### "WebSocket connection failed" in browser console

- Backend must be running on port 3011 (dev mode)
- Frontend dev server proxies `/ws` to the backend — check both are running
- Try restarting: `Ctrl+C` and re-run `./dev.sh`

### Frequent disconnections

- DreamForge sends a heartbeat every 30 seconds — if the connection drops between heartbeats, it reconnects automatically with exponential backoff (up to 30 seconds)
- If behind a reverse proxy (nginx, Caddy), ensure WebSocket upgrade is configured:
  ```nginx
  location /ws {
      proxy_pass http://localhost:3010;
      proxy_http_version 1.1;
      proxy_set_header Upgrade $http_upgrade;
      proxy_set_header Connection "upgrade";
      proxy_read_timeout 86400;
  }
  ```

### Messages not appearing after reconnect

- DreamForge has a replay buffer (last 500 messages)
- On reconnect, it replays messages from the last known sequence number
- If you missed more than 500 messages, refresh the page to reload the session

---

## Permissions

### Permission prompt stuck / not responding

- Permission prompts time out after 60 seconds (configurable via `DREAMFORGE_PERMISSION_TIMEOUT`)
- If the dialog doesn't appear, check browser console for errors
- Refresh the page and try again

### Agent keeps asking for permission

- Normal in `default` mode — the agent asks before writes and executes
- Switch to `accept_edits` to auto-approve file writes
- Use "Remember" checkbox with "tool" scope to stop repeat prompts for the same tool
- See [Permissions Guide](permissions-guide.md) for mode details

### "Permission denied" when agent should be allowed

- Check if there's a DENY rule in your permission config
- DENY rules cannot be overridden by any mode or grant
- Check org-policy.yaml if using organization policies
- Sensitive files (.env, .pem, .key, etc.) are always denied for writes

---

## Memory and Sessions

### Sessions not saving

- Sessions auto-save every 10 seconds
- Check that `DREAMFORGE_DATA_DIR` is writable
- Check disk space: `df -h`
- Session files are in `{DATA_DIR}/sessions/`

### Memory search returns nothing relevant

- Memory retrieval uses keyword matching (not semantic/vector search)
- Use specific terms in your queries
- Check Memory panel to verify memories exist
- Memory types have retrieval boosts: feedback > project > user > reference

### Corrupted session or memory files

- Session files are JSON in `{DATA_DIR}/sessions/`
- Memory files are markdown with YAML frontmatter in `{DATA_DIR}/memory/`
- DreamForge uses atomic writes (tempfile + os.replace) to prevent corruption
- If a file is corrupted, delete it — the session/memory will be lost but DreamForge will continue working

---

## Port Conflicts

### "Address already in use" on port 3010 or 3011

```bash
# Find what's using the port
lsof -i :3010
# or on Windows:
netstat -ano | findstr :3010

# Kill the process or use a different port:
DREAMFORGE_PORT=3012 ./dev.sh
```

Note: The backend runs on `DREAMFORGE_PORT` (default 3011 in dev mode). The frontend dev server runs on port 3010 and proxies API/WebSocket requests to the backend.

---

## Frontend

### Blank page after `npm run build`

- Check browser console for JavaScript errors
- Clear browser cache and hard reload (Ctrl+Shift+R)
- Verify `frontend/dist/` contains built files
- Re-run `npm run build` in the frontend directory

### "Module not found" errors during `npm install`

```bash
cd rust/frontend
rm -rf node_modules package-lock.json
npm install
```

### Styling looks broken

- Tailwind CSS may not have processed correctly
- Check that PostCSS and Tailwind configs exist in the frontend directory
- Re-run the dev server: `npx vite --port 3010`

---

## Docker-Specific Issues

### Container exits immediately

```bash
docker logs dream-dreamforge
```

Common causes:
- Missing environment variables
- `LLM_API_URL` pointing to unreachable host
- Port already in use

### "Permission denied" on mounted volumes

The container runs as user `forger` (UID 1000):

```bash
chown -R 1000:1000 ./data/dreamforge ./workspace
```

### Can't connect to LLM server from container

- Use Docker service names, not `localhost`: `LLM_API_URL=http://llama-server:8080`
- Ensure both containers are on the same Docker network
- Test from inside the container:
  ```bash
  docker exec dream-dreamforge curl http://llama-server:8080/v1/models
  ```

See [Docker Guide](docker-guide.md) for full Docker setup instructions.

---

## MCP Servers

### MCP server won't connect

- Verify the command exists: `which npx` (or the server command)
- Check that the server supports MCP protocol version 2024-11-05
- Look for errors in DreamForge logs (the server's stderr is captured)
- Make sure `.forge/settings.json` is valid JSON

### MCP tool calls timing out

- Default timeout: 30 seconds
- Increase per-server: add `"timeout": 60` in the server config
- Hard cap: 120 seconds
- Check if the MCP server itself is slow or unresponsive

See [MCP Configuration](mcp-configuration.md) for setup details.

---

## Getting More Help

- Check the [DreamForge README](../README.md) for architecture overview
- Review [Security Model](security-model.md) if something is being blocked
- Review [Permissions Guide](permissions-guide.md) for permission issues
- Check audit logs in `{DATA_DIR}/audit.log` for security events
- File issues at [github.com/Light-Heart-Labs/Dream-Forge/issues](https://github.com/Light-Heart-Labs/Dream-Forge/issues)
