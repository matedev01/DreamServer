# MCP Configuration Guide

## What Is MCP?

Model Context Protocol (MCP) is a standard for extending AI tools with external capabilities. DreamForge includes an MCP client that can connect to any MCP-compatible server, exposing the server's tools to the agent alongside the built-in tools.

MCP servers communicate via JSON-RPC 2.0 over stdio with Content-Length framing.

---

## Configuration File

MCP servers are defined in `.forge/settings.json`. Each key in the `servers` object is a unique name used to identify the server and prefix its tools.

```json
{
  "servers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"],
      "env": {},
      "timeout": 30
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..."
      },
      "timeout": 45
    },
    "sqlite": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-sqlite", "path/to/db.sqlite"],
      "env": {},
      "timeout": 30
    }
  }
}
```

---

## Server Configuration Fields

| Field     | Type     | Required | Description                                                    |
|-----------|----------|----------|----------------------------------------------------------------|
| `command` | string   | yes      | Executable path or command name                                |
| `args`    | string[] | yes      | Command-line arguments                                         |
| `env`     | object   | no       | Additional environment variables for the server process        |
| `timeout` | integer  | no       | Per-tool timeout in seconds (overrides the global default)     |

---

## How Tools Are Exposed

MCP tools are registered with a qualified name following this pattern:

```
mcp_{server_name}_{tool_name}
```

For example, if you add a server named `github` that exposes a tool called `create_issue`, it becomes available to the agent as `mcp_github_create_issue`.

This naming convention prevents collisions with built-in tools and makes it clear which server provides each tool.

---

## Timeouts

- **Global default:** 30 seconds (configurable via the `DREAMFORGE_MCP_TOOL_TIMEOUT` environment variable; see [configuration-reference.md](configuration-reference.md) for details)
- **Per-server override:** set the `timeout` field in the server config
- **Hard cap:** 120 seconds (cannot be exceeded regardless of configuration)

If a tool call exceeds its timeout, the process is terminated and an error is returned to the agent.

---

## Security

MCP tools go through the same 7-step security pipeline as built-in tools:

1. Schema validation
2. Permission evaluation (MCP tools default to EXECUTE access level, so they always prompt in default mode)
3. APE policy checks
4. Output truncation and secret scanning
5. Audit logging

For more on the security pipeline, see [security-model.md](security-model.md).

---

## Checking Server Status

Open **Settings > MCP Servers** to see:

- Which servers are configured
- Connection status (connected / disconnected)
- Tools exposed by each server

---

## Lifecycle

1. On startup, DreamForge reads `.forge/settings.json`.
2. For each server entry, DreamForge spawns the process, sends an `initialize` request (protocol version `2024-11-05`), and waits for a response.
3. Sends the `initialized` notification.
4. Fetches available tools from the server via `tools/list`.
5. Registers the tools in the tool registry with qualified names.

Servers run as long as DreamForge is running. If a server process crashes, its tools become unavailable until the next restart.

---

## Adding a New MCP Server

1. Find an MCP-compatible server (for example, from the MCP ecosystem or a custom implementation).
2. Add an entry to `.forge/settings.json`:
   ```json
   {
     "servers": {
       "my-server": {
         "command": "npx",
         "args": ["-y", "@example/mcp-server"],
         "env": {},
         "timeout": 30
       }
     }
   }
   ```
3. Restart DreamForge (servers are loaded at startup).
4. Verify the server shows as "connected" in **Settings > MCP Servers**.

---

## Protocol Details

| Property         | Value                                                                 |
|------------------|-----------------------------------------------------------------------|
| Transport        | stdio (stdin/stdout of the spawned process)                           |
| Framing          | Content-Length headers (HTTP-style)                                    |
| Max message size | 10 MB (prevents memory exhaustion from malicious servers)             |
| Protocol version | `2024-11-05`                                                          |

Message framing example:

```
Content-Length: 42\r\n\r\n{"jsonrpc":"2.0","method":"...","id":1}
```

---

## Troubleshooting

### Server won't connect

- Check that the command exists and is in your `PATH`.
- Check the server's logs (stderr output).
- Verify the server supports MCP protocol version `2024-11-05`.

### Tools not appearing

- Check **Settings > MCP Servers** for connection status.
- Make sure the server implements the `tools/list` method.
- Restart DreamForge after changing `.forge/settings.json`.

### Tool calls timing out

- Increase the `timeout` value in the server config.
- Check if the server is slow to respond (network latency, heavy computation).
- The hard cap is 120 seconds. If you need longer, consider breaking the operation into smaller steps.

---

## Related Documentation

- [configuration-reference.md](configuration-reference.md) -- environment variables including `DREAMFORGE_MCP_TOOL_TIMEOUT`
- [security-model.md](security-model.md) -- the 7-step security pipeline
- [tool-development.md](tool-development.md) -- building custom tools
