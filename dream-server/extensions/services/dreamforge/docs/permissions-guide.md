# Permissions Guide

Dream-Forge uses a layered permission system to control what the agent can do on your machine. Every tool call -- reading a file, writing code, running a command -- passes through the permission engine before it executes. This guide explains the permission prompts you see, how decisions are made, and how to customize the rules.

For related topics, see:

- [Security Model](security-model.md) -- threat model, containment, and audit logging
- [Configuration Reference](configuration-reference.md) -- all configuration options including permission YAML paths

---

## Permission Modes

Dream-Forge offers four permission modes that control how much autonomy the agent has. You can think of them as presets that shift the balance between safety and speed.

| Mode | Reads | File Writes | Bash / Execute | Best For |
|------|-------|-------------|----------------|----------|
| `default` | Auto-allowed | Prompts user | Prompts user | Normal interactive use |
| `plan` | Auto-allowed | Denied | Denied | Exploring and reviewing code safely |
| `accept_edits` | Auto-allowed | Auto-allowed | Prompts user | Trusting the agent with file changes |
| `full_auto` | Auto-allowed | Auto-allowed | Auto-allowed* | High-trust environments |

**Read operations are always allowed**, regardless of the active mode. This means the agent can always inspect your codebase, even in `plan` mode.

### A note on `full_auto`

`full_auto` removes most interactive prompts, but it is not unlimited. A built-in sanitizer automatically downgrades overly broad ALLOW rules to ASK. For example, even in `full_auto`, a rule like `tool="*" path="*" access="execute"` is downgraded so that the agent still asks for confirmation. See [Dangerous Rule Sanitization](#dangerous-rule-sanitization) below for the full list of patterns that trigger this safety net.

---

## Decision Flow

When the agent calls a tool, the permission engine evaluates the request in the following priority order:

```
1. Plan mode?            --> Block all non-read operations immediately.
2. Read operation?       --> Allow (always, in every mode).
3. DENY rule matches?    --> Block (absolute veto, cannot be overridden).
4. Full auto mode?       --> Allow (after deny rules have been checked).
5. Accept edits mode?    --> Allow file writes only.
6. Session grant exists? --> Allow if a matching grant is active and not expired.
7. Custom rules match?   --> Apply the highest-priority matching rule.
8. No match              --> ASK (prompt the user).
```

Key takeaway: **DENY rules always win.** No mode, grant, or custom rule can override a DENY. This is by design -- it lets you set hard boundaries that cannot be bypassed.

---

## Permission Prompts

When the agent needs your approval, you will see a dialog containing:

| Field | Description |
|-------|-------------|
| **Tool name** | Which tool wants to run (e.g., `bash`, `write_file`). |
| **Description** | What the tool wants to do (e.g., "Run: npm install"). |
| **Risk level** | Color-coded indicator: **high** (red), **medium** (yellow), **low** (green). |
| **Command or file path** | The specific action -- the shell command, the file being written, etc. |

You have three choices:

- **Allow** -- Let this specific action proceed.
- **Deny** -- Block this action. The agent is told the action was denied.
- **Remember** -- Save your decision so you are not asked again. You choose the scope:

| Remember Scope | Effect |
|----------------|--------|
| **Tool** | Applies to all future calls to this tool (e.g., all `bash` calls). |
| **Path** | Applies to calls matching a specific file path or fnmatch pattern. |
| **Session** | Applies to every tool call for the rest of this session (wildcard). |

---

## Session Grants

When you check "Remember" on a permission prompt, Dream-Forge creates a **session grant**. Session grants have these properties:

- **Scoped to the current session.** They do not persist across restarts.
- **Optional TTL (time-to-live).** A grant can automatically expire after a configurable number of seconds. Once expired, the engine falls through to the next evaluation step and may prompt you again.
- **Matched by tool and path.** Path-scoped grants use fnmatch patterns, so a grant for `src/**/*.py` covers all Python files under `src/`.

Session grants are evaluated at step 6 of the [decision flow](#decision-flow), after DENY rules and mode-level checks. This means a DENY rule always takes precedence over any grant you have saved.

---

## Switching Modes

There are two ways to change the active permission mode:

1. **In the UI:** Open the Settings page, go to the Permissions tab, and select a mode from the dropdown.
2. **Via WebSocket:** Send a `mode_change` message. See the [WebSocket Protocol](websocket-protocol.md) documentation for the message format.

Important details:

- Switching to `full_auto` requires explicit confirmation (`confirmed: true` in the WebSocket message, or a confirmation dialog in the UI). This prevents accidental escalation.
- Mode changes take effect immediately for the current session. There is no need to restart.
- Changing modes does not clear existing session grants or custom rules.

---

## Dangerous Rule Sanitization

In `full_auto` mode, the sanitizer catches overly broad rules that would effectively give the agent unrestricted access. The following patterns are automatically downgraded from ALLOW to ASK:

| Pattern | Why it is dangerous |
|---------|---------------------|
| `tool="*"` + `path="*"` + non-read access | Grants write or execute to every tool on every path. |
| `tool="*"` + execute access | Grants execute to every tool. |
| `tool="bash"` + `path="*"` + non-read access | Grants unrestricted shell access. |

These downgrades are logged for audit purposes so you can review what was caught. See [Security Model -- Audit Logging](security-model.md) for details on where logs are stored.

---

## Custom Permission Rules

You can define custom rules in YAML format to enforce project-specific or team-specific policies. Rules are loaded from the permission YAML file configured in your settings (see [Configuration Reference](configuration-reference.md) for the file path).

### Rule format

```yaml
rules:
  - tool: "bash"
    path_pattern: "*"
    access_level: "execute"
    decision: "deny"
    reason: "Bash disabled by policy"
    priority: 150

  - tool: "write_file"
    path_pattern: "**/*.md"
    access_level: "write"
    decision: "allow"
    reason: "Markdown files are safe to edit"
    priority: 120
```

### Rule fields

| Field | Required | Description |
|-------|----------|-------------|
| `tool` | Yes | Tool name to match, or `"*"` for all tools. |
| `path_pattern` | Yes | An fnmatch pattern matched against the file path or command. `"*"` matches everything. `"**/*.py"` matches all Python files in any subdirectory. |
| `access_level` | Yes | One of `"read"`, `"write"`, or `"execute"`. |
| `decision` | Yes | One of `"allow"`, `"deny"`, or `"ask"`. |
| `reason` | No | A human-readable explanation shown in prompts and logs. |
| `priority` | Yes | Integer. Higher number means higher priority. When multiple rules match, the one with the highest priority wins. |

### Priority guidelines

| Priority Range | Intended Use |
|----------------|-------------|
| 0 -- 99 | Low-priority defaults and fallbacks. |
| 100 -- 199 | Standard project rules. |
| 200 -- 299 | Security-critical DENY rules (e.g., blocking writes to secrets). |
| 300+ | Organization policies (see below). |

---

## Organization Policies

For teams and organizations, Dream-Forge supports org-wide rules defined in `org-policy.yaml`. These work exactly like custom rules but with a critical difference: **org rules have a minimum priority floor of 300**, ensuring they always override session-level rules.

Additionally, session ALLOW rules that conflict with org DENY rules are automatically filtered out at load time. This prevents individual users from granting themselves access to something the organization has explicitly blocked.

### Example org-policy.yaml

```yaml
rules:
  - tool: "bash"
    path_pattern: "*"
    decision: "deny"
    reason: "Org policy: bash disabled in this environment"
    priority: 350

  - tool: "write_file"
    path_pattern: "**/.github/**"
    decision: "ask"
    reason: "Org policy: CI config changes require approval"
    priority: 310
```

In this example:

- All shell execution is blocked organization-wide, regardless of what mode the user selects or what session grants they create.
- Changes to `.github/` files (CI/CD workflows, actions, etc.) always prompt for approval, even in `accept_edits` or `full_auto` mode.

See [Configuration Reference](configuration-reference.md) for the `org-policy.yaml` file path and loading behavior.

---

## Default Rules

Out of the box, Dream-Forge ships with these built-in rules:

| Access Level | Decision | Priority | Notes |
|-------------|----------|----------|-------|
| All reads | ALLOW | 100 | The agent can always read your codebase. |
| All writes | ASK | 0 | File modifications prompt you by default. |
| All executes | ASK | 0 | Shell commands prompt you by default. |
| Writes to sensitive files | DENY | 200 | Matches `.env*`, `*.pem`, `*.key`, `id_rsa*`. |

The sensitive-file DENY rule at priority 200 means that no standard project rule (priority 100--199) can override it. To allow writes to these files, you would need to define a rule at priority 201 or higher -- which should be a deliberate, reviewed decision.

---

## Denial Tracking

If you deny the same tool **three times consecutively**, Dream-Forge notices the pattern and suggests alternative approaches to the agent. Instead of the agent asking to run `bash` a fourth time, it will attempt a different strategy.

This feature exists to prevent the agent from getting stuck in a loop of repeated denied requests. It does not block the tool permanently -- the counter resets when a different tool is called or when you allow a request.

---

## Quick Reference

**I keep getting prompted for the same thing.**
Use the "Remember" option on the permission prompt and choose the appropriate scope (tool, path, or session).

**I want the agent to edit files but not run commands.**
Switch to `accept_edits` mode. File writes will be auto-approved while shell commands still prompt you.

**I want to block a specific tool entirely.**
Add a DENY rule in your custom rules YAML with `priority: 200` or higher.

**I am in `full_auto` but still getting prompted.**
The sanitizer is downgrading an overly broad rule. Check the audit log for details, and write more specific rules instead of wildcard patterns.

**My org policy is not being applied.**
Verify that the `org-policy.yaml` path is correct in your configuration and that all rules have a priority of 300 or higher. See [Configuration Reference](configuration-reference.md) for the relevant settings.
