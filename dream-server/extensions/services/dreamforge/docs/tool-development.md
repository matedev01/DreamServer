# Tool Development Guide

## Overview

DreamForge tools are Python classes that extend `BaseTool`. Every tool call goes through a 7-step security pipeline before execution:

1. **Schema validation** — checks required parameters
2. **Security check** — shell parser (bash) or path validator (file tools)
3. **Permission evaluation** — declarative rules + user prompts via WebSocket
4. **Pre-hooks** — APE policy engine (optional)
5. **Execution** — your tool's `execute()` method
6. **Post-hooks** — output truncation for oversized results
7. **Audit logging** — structured JSON event logging

## Creating a New Tool

### 1. Subclass `BaseTool`

Create a file in `tools/builtin/`:

```python
"""My tool — brief description of what it does."""

from __future__ import annotations

from typing import Any

from models.tools import ToolAccessLevel, ToolParameter, ToolResult
from tools.base import BaseTool, ToolContext


class MyTool(BaseTool):
    name = "my_tool"
    description = "One-line description shown to the LLM."
    access_level = ToolAccessLevel.READ  # or WRITE, EXECUTE

    def get_parameters(self) -> list[ToolParameter]:
        return [
            ToolParameter(
                name="input",
                type="string",
                description="What this parameter does",
                required=True,
            ),
            ToolParameter(
                name="optional_flag",
                type="boolean",
                description="Optional behavior flag",
                required=False,
                default=False,
            ),
        ]

    async def execute(self, args: dict[str, Any], ctx: ToolContext) -> ToolResult:
        input_val = args["input"]
        # ... your logic here ...

        return ToolResult(
            tool_call_id="",  # populated by pipeline
            name=self.name,
            content="Result text shown to the LLM",
        )
```

### 2. Access Levels

| Level | Behavior | Examples |
|-------|----------|---------|
| `READ` | No permission prompt in default mode | `read_file`, `list_directory` |
| `WRITE` | Prompts in default mode, auto-approved in `accept_edits` | `write_file`, `edit_file` |
| `EXECUTE` | Always prompts unless `full_auto` mode | `bash`, MCP tools |

### 3. Register the Tool

In `tools/registry.py`, add your tool to the registration list:

```python
from tools.builtin.my_tool import MyTool

# In the register_builtins() function:
registry.register(MyTool())
```

### 4. ToolContext

The `ctx` object passed to `execute()` provides:

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | str | Current session ID |
| `working_directory` | str | Workspace path |
| `resolved_path` | str or None | Security-resolved file path (prevents TOCTOU) |
| `permission_mode` | str | Current permission mode |

**Important:** For file tools, always use `ctx.resolved_path` when available. The security engine pre-resolves paths to prevent time-of-check-time-of-use attacks.

### 5. Error Handling

Return errors via `ToolResult` with `is_error=True`:

```python
return ToolResult(
    tool_call_id="",
    name=self.name,
    content=f"Error: {description}",
    is_error=True,
)
```

For transient I/O errors, use the `retry_io` utility:

```python
from tools.utils import retry_io

text = await retry_io(lambda: path.read_text(encoding="utf-8"))
```

### 6. Security Considerations

- **Shell commands:** The `bash` tool's commands go through `shell_parser.py` for injection detection. If your tool runs subprocess commands, route them through the same parser.
- **File paths:** File tools have paths validated by `path_validator.py` — workspace containment, symlink resolution, sensitive file blocking.
- **Output size:** Results exceeding `TOOL_RESULT_MAX_CHARS` (50K) are automatically truncated by the pipeline. Set `truncated=True` on the result if you handle truncation yourself.

## Example: Walkthrough of `read_file`

See `tools/builtin/read_file.py`:

1. Extracts `file_path`, `offset`, `limit` from args
2. Uses `ctx.resolved_path` (pre-validated by security engine)
3. Checks file exists and is a regular file
4. Reads text with `retry_io` for transient error resilience
5. Slices lines based on offset/limit
6. Returns numbered lines as text content

## Testing

Add tests in `tests/`. Follow the existing pattern:

```python
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tools.builtin.my_tool import MyTool

class TestMyTool:
    def test_basic_functionality(self):
        # Test your tool's logic
        pass
```

Run tests: `python -m pytest tests/ -v`
