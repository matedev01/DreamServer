# Security Model

This document describes how Dream-Forge protects your system from unintended or malicious actions. It is written for both end users who want to understand what the agent can and cannot do, and for security reviewers performing due diligence.

For related documentation, see:

- [Permissions Guide](permissions-guide.md) -- declarative permission rules and user prompt flows
- [Configuration Reference](configuration-reference.md) -- all security-related environment variables and settings

---

## Security Philosophy

Dream-Forge follows two core principles:

1. **Defense in depth.** No single layer is trusted to be sufficient. Multiple independent checks guard every action the agent takes.
2. **Fail-closed by default.** If any layer is uncertain, the action is denied. The agent must prove an operation is safe rather than prove it is dangerous.

Every tool call -- whether it runs a shell command, writes a file, or calls an external API -- passes through a strict 7-step pipeline before execution. The pipeline is enforced by the runtime, not by the LLM. Even if the language model produces a malicious tool call, the pipeline blocks it before it reaches your system.

---

## The 7-Step Tool Pipeline

Every tool invocation flows through these stages in order. A failure at any stage halts the pipeline immediately.

| Step | Name | What It Does |
|------|------|-------------|
| 1 | **Schema Validation** | Checks that all required parameters are present and correctly typed. Rejects malformed calls before any further processing. |
| 2 | **Security Check** | Runs the shell parser (for bash/zsh/PowerShell commands) or the path validator (for file tools). Detects injection patterns, blocked commands, and path traversal. |
| 3 | **Permission Evaluation** | Evaluates declarative permission rules. If no rule matches, prompts the user for a decision via WebSocket. See the [Permissions Guide](permissions-guide.md) for details on rule syntax. |
| 4 | **Pre-hooks** | Invokes the APE (Agent Policy Engine) if configured. This is an optional external policy service that can apply organization-wide rules. |
| 5 | **Execution** | Calls the tool's `execute()` method. This is the only step that performs the actual operation. |
| 6 | **Post-hooks** | Truncates oversized output and runs secret scanning/redaction on the result before it is returned to the LLM. |
| 7 | **Audit Logging** | Writes a structured JSON event to the audit log, including the tool name, parameters, verdict, and outcome. |

---

## Shell Command Security

When the agent attempts to run a shell command, the security check (Step 2) parses the command and assigns one of three verdicts:

| Verdict | Behavior |
|---------|----------|
| **SAFE** | Auto-allowed. The command is read-only with no side effects. |
| **DANGEROUS** | Always blocked. The command is never executed regardless of user preference. |
| **ASK** | The user is prompted via the WebSocket interface and must explicitly approve or deny. |

### Auto-Allowed Commands (SAFE)

These commands are read-only and produce no side effects:

```
ls    cat    head    tail    grep    rg      find      wc
echo  pwd    which   file    stat    du      df        tree
printenv      date   uname   whoami  hostname  id
sort  uniq   tr      cut     less    more    diff
md5sum        sha256sum      realpath        dirname   basename
test  true   false
```

In addition, over 150 commands are classified with **per-flag granularity**. For example:

- `sed` (without `-i`) is SAFE -- it only prints to stdout.
- `sed -i` is ASK -- it modifies files in place.
- `git status` is SAFE -- read-only repository state.
- `git push` is ASK -- it sends commits to a remote.

### Always-Blocked Commands (DANGEROUS)

These patterns are rejected unconditionally. No user override is possible.

**Injection patterns:**

- Command substitution: `$(...)`, backticks
- Process substitution: `<(...)`, `>(...)`
- Indirect expansion, `eval`, `exec`
- IFS manipulation: `IFS=`, `export IFS`
- Hex/octal escape sequences
- ANSI-C quoting: `$'...'`

**Destructive system commands:**

- `rm -rf /`, `mkfs`, `dd`, `shred`, `format`, `fdisk`
- `reboot`, `shutdown`, `mount`, `chroot`

**Remote code execution:**

- `curl | bash`, `wget | sh`
- `nc -e`, `python -c`, `node -e`, `perl -e`, `bash -i`

**System control:**

- `systemctl start/stop`, `iptables`
- `chmod` with world-writable permissions

### User-Prompted Commands (ASK)

These require explicit user approval each time:

- **Network:** `curl`, `wget`, `ssh`, `scp`, `rsync`
- **Destructive git:** `push`, `reset --hard`, `clean -f`
- **Containers:** `docker run`, `docker exec`
- **Package management:** `cargo install`, `npm publish`
- **File deletion:** `rm` (without the always-blocked `rm -rf /` pattern)

---

## 15-Layer Injection Defense

The shell parser applies 15 independent detection layers to catch obfuscation and injection attempts. Each layer targets a specific attack class.

| Layer | Attack Class | Examples |
|-------|-------------|----------|
| 1 | Brace expansion | `{/etc/passwd,/etc/shadow}`, large range DoS `{1..999999}` |
| 2 | Unicode whitespace obfuscation | 29+ non-ASCII space characters, zero-width joiners, bidirectional control characters |
| 3 | IFS injection | `IFS=`, `export IFS`, `$IFS` |
| 4 | Control character injection | Null bytes, C0/C1 control codes, ANSI escape sequences |
| 5 | Quote desynchronization | Mismatched or unclosed quotes |
| 6 | Quoted newline injection | Newlines embedded inside quoted strings |
| 7 | Backslash-escaped operators | `\|`, `\;`, `\&` used to smuggle operators |
| 8 | ANSI-C quoting | `$'\x41'` and similar escape sequences |
| 9 | Hex/octal escapes | `\x2f`, `\057`, and other encoded characters |
| 10 | Incomplete command detection | Trailing `&&`, `\|\|`, `\|`, `;` |
| 11 | JQ system() calls | `jq` expressions containing `system()` or `input` |
| 12 | Obfuscated flags | Variable expansion in flag position (`$FLAG`) |
| 13 | /proc and /dev access | `/proc/*/environ`, `/proc/*/cmdline`, `/proc/*/mem`, `/dev/tcp`, `/dev/udp` |
| 14 | Git commit substitution | Malicious payloads hidden in commit messages or refs |
| 15 | Environment/PATH manipulation | `PATH=`, `LD_PRELOAD=`, `BASH_ENV=`, `PROMPT_COMMAND=` |

---

## Cross-Platform Shell Protections

### ZSH

Dream-Forge blocks **24 dangerous ZSH builtins**, including:

`zmodload`, `emulate`, `ztcp`, `zsocket`, `zpty`, `sysopen`, `sysread`, `syswrite`, `bindkey -s`, `sched`, `autoload`

It also blocks **6 ZSH-specific syntax patterns**:

- Glob qualifiers with execution (e.g., `*(e:'...':)`)
- Anonymous functions
- Process substitution with temp files

### PowerShell

Dream-Forge blocks **18 dangerous PowerShell patterns**, including:

- `Invoke-Expression` / `iex`
- `Set-ExecutionPolicy`
- `Remove-Item -Recurse -Force`
- `Format-Volume`
- `Stop-Computer`
- Registry modification (`Set-ItemProperty HKLM:`, etc.)
- `Invoke-WebRequest | Invoke-Expression`
- `-EncodedCommand`

---

## File Security

### Sensitive File Patterns

Writes to files matching these patterns are **always blocked**:

| Category | Patterns |
|----------|----------|
| Environment/config | `.env`, `.env.*`, `*.env` |
| Credentials | `*credentials*`, `*secret*`, `*token*` |
| Certificates and keys | `*.pem`, `*.key`, `*.pfx`, `*.p12`, `*.crt` |
| SSH keys | `id_rsa*`, `id_ed25519*`, `id_ecdsa*` |
| Git/SSH config | `.git/config`, `.ssh/config` |
| System auth | `shadow`, `passwd`, `sudoers` |
| Password databases | `*.kdbx` |
| SSH trust | `authorized_keys`, `known_hosts` |

### Protected System Directories

Writes to these directories are **always blocked**:

| Platform | Directories |
|----------|------------|
| Unix / macOS | `/etc`, `/usr`, `/bin`, `/sbin`, `/lib`, `/lib64`, `/boot`, `/proc`, `/sys`, `/dev`, `/var/log`, `/var/run` |
| Windows | `C:\Windows`, `C:\Program Files`, `C:\Program Files (x86)`, `C:\ProgramData` |

### Path Validation

All file paths are validated before any operation:

- **Workspace containment:** For write operations, the resolved path must be within the workspace directory. Writes outside the workspace are denied.
- **Symlink resolution:** All paths are resolved to their real location to prevent TOCTOU (time-of-check-time-of-use) attacks. Symlinks targeting locations outside the workspace are rejected for writes.
- **Symlink rejection for writes:** Write operations must target real paths, not symlinks. This prevents an attacker from creating a symlink that points to a sensitive location.
- **Read containment:** The `READ_CONTAINMENT` setting (see [Configuration Reference](configuration-reference.md)) controls whether reads are restricted to the workspace (`"workspace"`) or allowed system-wide (`"system"`).

---

## Secret Scanning

All tool output is scanned for credential patterns **before** being returned to the LLM. This prevents accidental exfiltration of secrets through the model's context window.

Dream-Forge detects **46 credential patterns** across the following categories:

| Category | Patterns Detected |
|----------|------------------|
| Cloud providers | AWS Access Key ID, AWS Secret Access Key, GCP Service Account Key |
| Code hosting | GitHub Token, GitLab PAT, Bitbucket App Password |
| AI services | Anthropic API Key, OpenAI API Key |
| Communication | Slack Token, Discord Bot Token |
| Payment | Stripe Key, Twilio Token |
| Infrastructure | npm Token, DigitalOcean Token, Sentry DSN |
| Cryptographic keys | RSA, EC, DSA, and OpenSSH Private Keys, PGP Private Key Blocks |
| Generic | Bearer Tokens, Basic Auth headers, Database Connection Strings, Passwords in URLs, API Keys, JWTs |

When a secret is detected, it is replaced with `[REDACTED:PatternName]` in the tool output. The original value never reaches the LLM.

Secret scanning is enabled by default and can be controlled with the `DREAMFORGE_SECRET_SCANNING` environment variable. See [Configuration Reference](configuration-reference.md) for details.

---

## API Authentication

Dream-Forge authenticates all API and WebSocket connections:

| Mechanism | Details |
|-----------|---------|
| REST API | Bearer token in the `Authorization` header |
| WebSocket | Token passed as a query parameter (`/ws?token=...`) |
| Token comparison | HMAC `compare_digest` for constant-time comparison, preventing timing side-channel attacks |
| Key generation | `secrets.token_hex(32)` -- 256 bits of cryptographic randomness |
| Key storage | File permissions set to `0o600` on Unix (owner read/write only) |

---

## Audit Logging

Every security-relevant event is written to a structured audit log.

| Property | Value |
|----------|-------|
| Format | JSON Lines (one JSON object per line) |
| Location | `{DATA_DIR}/audit.log` |
| Rotation | At 10 MB, with 5 backup files retained |
| Toggle | `DREAMFORGE_AUDIT_LOG` environment variable (default: `true`) |

### Logged Event Types

| Event | Description |
|-------|------------|
| `tool_execution` | A tool was invoked, including its parameters and result status |
| `security_block` | A tool call was blocked by the security check or injection defense |
| `permission_decision` | A user approved or denied a prompted action |
| `auth_failure` | An API or WebSocket authentication attempt failed |
| `mode_change` | The agent's operating mode was changed |
| `secret_redaction` | A secret was detected and redacted from tool output |

---

## Rate Limiting

Rate limiting prevents runaway loops and resource exhaustion from malfunctioning or adversarial tool calls.

| Setting | Default | Description |
|---------|---------|------------|
| Global rate limit | 60 calls/minute/session | Maximum tool invocations per minute for a single session |
| Per-tool overrides | None | JSON map of tool name to custom rate limit, configured via `DREAMFORGE_TOOL_RATE_OVERRIDES` |

When the rate limit is exceeded, subsequent tool calls are rejected until the window resets. See [Configuration Reference](configuration-reference.md) for override syntax.

---

## Summary

Dream-Forge enforces security at every layer of the tool execution pipeline. The LLM has no mechanism to bypass these controls -- they are enforced by the runtime before and after every tool call. Shell commands are parsed and classified with 15 independent injection detection layers. File operations are contained to the workspace. Secrets are redacted before they reach the model. All actions are logged for audit.

For configuration options, see [Configuration Reference](configuration-reference.md). For permission rule syntax and user prompt behavior, see [Permissions Guide](permissions-guide.md).
