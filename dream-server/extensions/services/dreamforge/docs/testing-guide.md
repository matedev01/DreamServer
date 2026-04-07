# Testing Guide

How to run existing tests and write new ones for DreamForge.

---

## Running Tests
### Backend (Rust)

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p dreamforge-security

# Run a specific test
cargo test -p dreamforge-security test_shell_parser

# Run with verbose output
cargo test --workspace -- --nocapture

# Run only tests matching a keyword
cargo test --workspace -- security
```

### Frontend (JavaScript)

```bash
cd rust/frontend

# Run all tests (single run)
npm test

# Run in watch mode (re-runs on file changes)
npm run test:watch
```

Frontend tests use [Vitest](https://vitest.dev/) with jsdom environment and [@testing-library/react](https://testing-library.com/docs/react-testing-library/intro/).

---

## Test Organization

### Backend Tests

All backend tests are in `rust/crates/*/tests/`. There are 40+ test files organized by area:

| Category | Key Files | What They Test |
|----------|-----------|----------------|
| Security | `deep_security.rs`, `adversarial_security.rs`, `shell_security.rs`, `shell_parser.rs` | Shell injection defense, command classification, read-only rules |
| Permissions | `permission_engine.rs`, `permission_sanitizer.rs`, `deep_permissions.rs` | Mode behavior, session grants, rule evaluation, dangerous rule sanitization |
| Tools | `tool_pipeline.rs`, `phase1_tools.rs`, `final_tools.rs`, `tool_depth.rs` | Tool execution, pipeline steps, individual tool behavior |
| Agent | `deep_agent.rs`, `e2e.rs` | Query loop, WebSocket message flow, rate limiting |
| MCP | `deep_mcp.rs` | Transport config, resource loading, sampling |
| Models | `models_router.rs` | Model validation |
| Paths | `path_validator.rs` | Workspace containment, sensitive files, symlinks |
| Features | `new_features.rs`, `bug_fixes.rs` | Specific features and regression tests |

### Frontend Tests

Frontend tests are in `rust/frontend/src/components/__tests__/`:

| File | What It Tests |
|------|---------------|
| `MessageBubble.test.jsx` | Message rendering, markdown, code blocks |
| `ModeSwitch.test.jsx` | Permission mode selector |
| `StatusBar.test.jsx` | Connection status, model display |

---

## Writing a New Backend Test

### Basic Pattern

Tests live alongside the code in each crate under `rust/crates/*/tests/` or as inline `#[cfg(test)]` modules:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_functionality() {
        let tool = MyTool::new();
        assert_eq!(tool.name(), "my_tool");
        assert_eq!(tool.access_level(), AccessLevel::Read);
    }

    #[test]
    fn test_parameter_validation() {
        let tool = MyTool::new();
        let params = tool.parameters();
        assert!(!params.is_empty());
        assert!(params[0].required);
    }
}
```

### Parametrized Tests

Use the `rstest` crate or a loop for testing multiple inputs:

```rust
use rstest::rstest;

#[rstest]
#[case("echo hello", FlagVerdict::Read)]
#[case("ls -la", FlagVerdict::Read)]
#[case("rm file.txt", FlagVerdict::Execute)]
#[case("sed -i 's/a/b/' file.txt", FlagVerdict::Write)]
fn test_command_classification(#[case] cmd: &str, #[case] expected: FlagVerdict) {
    let result = evaluate_command(cmd);
    assert_eq!(result, expected);
}
```

### Async Tests

For testing async functions, use `#[tokio::test]`:

```rust
#[tokio::test]
async fn test_async_tool_execution() {
    let tool = MyTool::new();
    let ctx = ToolContext {
        working_directory: PathBuf::from("/workspace"),
        session_id: "test-session".into(),
        abort_token: CancellationToken::new(),
    };
    let result = tool.execute(json!({"input": "test"}), &ctx).await;
    assert!(!result.is_error);
}
```

### Testing Security Rules

```rust
use dreamforge_security::shell_parser::{parse_command, SecurityVerdict};

#[test]
fn test_safe_command() {
    let result = parse_command("echo hello");
    assert_eq!(result.verdict, SecurityVerdict::Safe);
}

#[test]
fn test_dangerous_injection() {
    let result = parse_command("echo $(cat /etc/passwd)");
    assert_eq!(result.verdict, SecurityVerdict::Dangerous);
}

#[test]
fn test_ask_command() {
    let result = parse_command("curl https://example.com");
    assert_eq!(result.verdict, SecurityVerdict::Ask);
}
```

---

## Writing a New Frontend Test

### Basic Component Test

```jsx
import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import MyComponent from '../MyComponent'

describe('MyComponent', () => {
  it('renders correctly', () => {
    render(<MyComponent title="Test" />)
    expect(screen.getByText('Test')).toBeDefined()
  })

  it('handles click events', async () => {
    const onClick = vi.fn()
    render(<MyComponent onClick={onClick} />)
    await screen.getByRole('button').click()
    expect(onClick).toHaveBeenCalledOnce()
  })
})
```

### Testing with ForgeContext

Components that use `useForge()` need to be wrapped in the context provider:

```jsx
import { ForgeProvider } from '../../contexts/ForgeContext'

function renderWithContext(component) {
  return render(
    <ForgeProvider>
      {component}
    </ForgeProvider>
  )
}

it('shows connection status', () => {
  renderWithContext(<StatusBar />)
  // ... assertions
})
```

### Test Configuration

Frontend tests are configured in `vite.config.js`:

```javascript
test: {
  environment: 'jsdom',
  globals: true,
  setupFiles: './src/test-setup.js',
}
```

The `test-setup.js` file configures the test environment (e.g., DOM mocks).

---

## Test Conventions

- **File naming:** `{module_name}.rs` or `test_{module_name}.rs` for backend, `{Component}.test.jsx` for frontend
- **Class naming:** `TestFeatureName` (PascalCase, prefixed with Test)
- **Method naming:** `test_what_it_tests` (snake_case, prefixed with test_)
- **One assertion per concept** — each test should verify one behavior
- **No external dependencies** — tests should not require a running LLM server, database, or network access
- **Module imports — each test module uses standard Rust `use` declarations

---

## What to Test When Contributing

- **New tool:** Test parameter schema, execute with valid/invalid inputs, error handling
- **Security change:** Test both blocked and allowed patterns, edge cases
- **Permission change:** Test all 4 modes, grant/deny behavior
- **Frontend component:** Test rendering, user interaction, context integration
- **Bug fix:** Add a regression test that would have caught the original bug
