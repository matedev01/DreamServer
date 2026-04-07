# Docker Guide

How to run DreamForge with Docker, either standalone or as a DreamServer extension.

---

## Quick Start with Docker Compose

### 1. Build the Image

```bash
cd Dream-Forge

# Build the frontend first
cd rust/frontend
npm install && npm run build
cd ../../../..

# Build the Docker image
docker build -t dreamforge rust/
```

### 2. Run with Docker Compose

```bash
docker compose -f compose.yaml up -d
```

Open **http://localhost:3010** in your browser.

### 3. Stop

```bash
docker compose -f compose.yaml down
```

---

## Running as a DreamServer Extension

Copy the DreamForge extension into your DreamServer install:

```bash
cp -r rust/ /path/to/dreamserver/extensions/services/dreamforge/
```

It registers automatically via the manifest system. Start it with:

```bash
dream-cli start dreamforge
```

DreamForge connects to DreamServer's service mesh and auto-discovers llama-server and optional services.

---

## compose.yaml Explained

```yaml
dreamforge:
  build: ./rust
  container_name: dream-dreamforge
  restart: unless-stopped
  security_opt:
    - no-new-privileges:true
```

### Volumes

```yaml
volumes:
  - ./data/dreamforge:/data/dreamforge    # Persistent data (sessions, memory, API key)
  - ${DREAMFORGE_HOST_WORKSPACE:-./workspace}:/workspace  # Your code
```

- **`/data/dreamforge`** — sessions, memory files, API key, audit logs. Mount this to persist data across container restarts.
- **`/workspace`** — the directory the agent works in. Map this to your project directory. Override with `DREAMFORGE_HOST_WORKSPACE` env var.

### Ports

```yaml
ports:
  - 127.0.0.1:3010:3010
```

Bound to localhost only by default. To expose externally (not recommended without auth proxy):

```yaml
ports:
  - 0.0.0.0:3010:3010
```

### Resource Limits

```yaml
deploy:
  resources:
    limits:
      cpus: "4.0"
      memory: 4G
    reservations:
      cpus: "0.5"
      memory: 512M
```

DreamForge itself is lightweight. The LLM server is what needs GPU resources — DreamForge only sends API requests to it.

### Networking

```yaml
networks:
  - dream-network
```

Uses an external Docker network (`dream-network`) for communication with other DreamServer services. Create it if it doesn't exist:

```bash
docker network create dream-network
```

### Health Check

```yaml
healthcheck:
  test: ["CMD", "curl", "-f", "http://localhost:3010/health"]
  interval: 30s
  timeout: 10s
  retries: 3
  start_period: 10s
```

The container reports healthy once the `/health` endpoint responds.

---

## Environment Variables

Pass environment variables in compose.yaml or via a `.env` file:

```yaml
environment:
  - LLM_API_URL=http://llama-server:8080
  - DREAMFORGE_PERMISSION_MODE=default
  - DREAMFORGE_API_KEY=your-secret-key-here
```

Or mount the `.env.example` file:

```bash
cp .env.example .env
# Edit .env with your values
docker compose --env-file .env -f compose.yaml up -d
```

See [Configuration Reference](configuration-reference.md) for all available variables.

### Key Variables for Docker

| Variable | Docker Default | Notes |
|----------|---------------|-------|
| `LLM_API_URL` | `http://llama-server:8080` | Use container name if on same Docker network |
| `DREAMFORGE_WORKSPACE` | `/workspace` | Matches the volume mount |
| `DREAMFORGE_DATA_DIR` | `/data/dreamforge` | Matches the volume mount |
| `DREAMFORGE_API_KEY` | (auto-generated) | Set explicitly for production |
| `GPU_BACKEND` | `nvidia` | `nvidia`, `amd`, `apple`, or `cpu` |

---

## GPU Backend Selection

Set `GPU_BACKEND` to match your hardware:

| Value | Hardware | Notes |
|-------|----------|-------|
| `nvidia` | NVIDIA GPUs | Default. Uses CUDA. |
| `amd` | AMD GPUs | Uses ROCm. |
| `apple` | Apple Silicon | Uses Metal. macOS only. |
| `cpu` | No GPU | Slowest, but works everywhere. |

This setting is passed to the LLM server (when managed by DreamServer) to select the right inference backend.

---

## Optional Services

DreamForge integrates with several optional services. Only **llama-server** is required — everything else degrades gracefully if unavailable.

| Service | Default URL | Purpose | Without It |
|---------|-------------|---------|------------|
| llama-server | `http://llama-server:8080` | LLM inference | **Required** — DreamForge won't work |
| Token Spy | `http://token-spy:8080` | Token usage tracking | Usage stats unavailable |
| APE | `http://ape:7890` | Policy enforcement | Falls back to local rules only |
| Qdrant | `http://qdrant:6333` | Vector memory (RAG) | Keyword-only memory retrieval |
| SearXNG | `http://searxng:8080` | Web search | web_search tool unavailable |
| Dashboard API | `http://dashboard-api:3002` | GPU metrics | No GPU stats in UI |
| Embeddings | `http://embeddings:80` | Embedding model | RAG document ingestion unavailable |
| Whisper | `http://whisper:8000` | Speech-to-text | Voice input unavailable |
| TTS | `http://tts:8880` | Text-to-speech | Voice output unavailable |

To enable an optional service, make sure it's running on the same Docker network and set its URL environment variable.

---

## Building the Image Locally

The Dockerfile is a multi-stage build based on Rust and Node:

```bash
docker build -t dreamforge rust/
```

What's installed:
- System packages: `curl`, `git`, `ripgrep` (used by tools)
- A non-root user `forger` (UID 1000) runs the application
- Pre-built frontend served as static files from `/app/static/`

**Important:** Build the frontend (`npm run build`) before building the Docker image. The Dockerfile copies `frontend/dist/` into the image.

---

## Monitoring

### Health Checks

- **`GET /health`** — basic health (no auth required)
- **`GET /readyz`** — readiness check (verifies LLM model configured)

### Logs

```bash
# Follow container logs
docker logs -f dream-dreamforge

# Check audit log (inside container)
docker exec dream-dreamforge cat /data/dreamforge/audit.log
```

### Resource Usage

```bash
docker stats dream-dreamforge
```

---

## Troubleshooting Docker

### Container starts but UI shows "Connection failed"

- Check that port 3010 is mapped correctly
- Verify the container is healthy: `docker inspect --format='{{.State.Health.Status}}' dream-dreamforge`
- Check container logs: `docker logs dream-dreamforge`

### "Cannot connect to LLM server"

- Make sure llama-server (or your LLM) is on the same Docker network
- Use the container name (not localhost) in `LLM_API_URL`
- Test from inside the container: `docker exec dream-dreamforge curl http://llama-server:8080/v1/models`

### Permission denied on volume mounts

- The container runs as user `forger` (UID 1000)
- Ensure your mounted directories are writable by UID 1000
- Or run: `chown -R 1000:1000 ./data/dreamforge ./workspace`

### Frontend shows blank page

- Make sure the frontend was built before the Docker image: `cd frontend && npm run build`
- Check that `frontend/dist/` exists and contains files

See [Troubleshooting](troubleshooting.md) for more common issues.
