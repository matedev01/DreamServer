# Frontend Development Guide

Architecture and development guide for the DreamForge React frontend.

---

## Tech Stack

- **React 18.3** — UI framework
- **Vite 5.4** — build tool and dev server
- **Tailwind CSS 3.4** — utility-first styling
- **Lucide React** — icon library
- **React Markdown** — markdown rendering in messages
- **React Syntax Highlighter** — code block syntax highlighting
- **Vitest** — test framework

---

## Project Structure

```
frontend/
  src/
    main.jsx                 # Entry point — renders App into DOM
    App.jsx                  # Root component — wraps everything in ForgeProvider
    index.css                # Tailwind imports + custom styles
    apiFetch.js              # REST API helper (adds auth header)
    components/
      AgentVisualization.jsx # Agent state visualization
      ChatPanel.jsx          # Main chat interface (messages + input)
      CodeBlock.jsx          # Syntax-highlighted code blocks
      CodeEditor.jsx         # Integrated code editor panel
      CommandPalette.jsx     # Command palette (keyboard-driven)
      DiffViewer.jsx         # Unified diff display for file edits
      DocumentPanel.jsx      # Document viewer side panel
      EditorPanel.jsx        # Editor panel container
      FileTreeBrowser.jsx    # Workspace file tree
      MemoryPanel.jsx        # Memory CRUD panel
      MessageBubble.jsx      # Individual message rendering
      ModeSwitch.jsx         # Permission mode selector
      OnboardingWizard.jsx   # First-run walkthrough
      PermissionDialog.jsx   # Permission request dialog
      SessionSidebar.jsx     # Session list and management
      SettingsPage.jsx       # Settings with tabs
      StatusBar.jsx          # Top bar (model, connection, tokens)
      StreamingMarkdown.jsx  # Streaming markdown renderer
      ToolCallCard.jsx       # Tool execution display card
      ToolCallVisualization.jsx # Tool call visual indicator
      TTSButton.jsx          # Text-to-speech playback
      VirtualMessageList.jsx # Virtualized message list (performance)
      VoiceButton.jsx        # Voice input recording
      __tests__/             # Component tests
    contexts/
      ForgeContext.jsx       # Global state + WebSocket management
    hooks/
      useKeyboardShortcuts.js # App-wide keyboard shortcuts
      useVoice.js            # Audio recording and transcription
  package.json
  vite.config.js
  tailwind.config.js
  postcss.config.js
```

---

## Component Tree

```
App
└── ForgeProvider (context + WebSocket)
    └── AppContent
        ├── StatusBar
        ├── SessionSidebar
        ├── Main Area
        │   ├── EditorPanel (Ctrl+Shift+E)
        │   └── ChatPanel
        │       ├── VirtualMessageList
        │       │   ├── MessageBubble (user/assistant)
        │       │   └── ToolCallCard (tool calls)
        │       ├── Input textarea
        │       ├── VoiceButton (Ctrl+Shift+V)
        │       └── TTSButton
        ├── Side Panels
        │   ├── MemoryPanel (Ctrl+Shift+M)
        │   └── DocumentPanel (Ctrl+Shift+D)
        ├── SettingsPage (Ctrl+/)
        │   └── ModeSwitch
        ├── PermissionDialog (modal overlay)
        ├── CommandPalette
        └── OnboardingWizard (first visit)
```

---

## State Management: ForgeContext

All application state lives in `ForgeContext.jsx`. There is no Redux or external state library — everything flows through React context.

### Key State

```javascript
{
  connected: boolean,          // WebSocket connection status
  messages: Array,             // Chat messages (user, assistant, tool, error, system)
  agentStatus: string,         // 'idle' | 'running' | 'waiting_permission'
  model: string,               // Current LLM model name
  session: Object,             // Session info (id, turn_count, etc.)
  pendingPermission: Object,   // Active permission dialog data
  tokenUsage: Object,          // { pct, totalIn, totalOut, remaining }
}
```

### WebSocket Message Flow

The context manages a WebSocket connection to `ws://localhost:3010/ws?token=<apikey>`. When a message arrives, it updates state based on the message type:

| Server Message | State Update |
|---------------|-------------|
| `session_info` | Set model, session, clear messages on switch |
| `status` | Update agentStatus |
| `assistant_text` | Append/stream into current assistant message |
| `assistant_text_done` | Finalize assistant message |
| `tool_call_start` | Add tool message (role=tool, type=start) |
| `tool_call_result` | Update tool message with result |
| `turn_complete` / `query_complete` | Set agentStatus to idle |
| `token_usage` | Update usage percentages |
| `permission_request` | Set pendingPermission, show dialog |
| `compaction_notice` | Add system message |
| `error` | Add error message |
| `heartbeat` | No-op (keep-alive) |

### Sending Messages

Components use context methods to send WebSocket messages:

```javascript
const { sendMessage, abort, respondToPermission } = useForge()

// Send a user message
sendMessage("Read the file README.md")

// Abort the current query
abort()

// Respond to a permission prompt
respondToPermission(requestId, { granted: true, remember: true, scope: "tool" })
```

### Reconnection

On disconnect, the context reconnects with exponential backoff (up to 30 seconds). On reconnect, it sends a `session_switch` message with the last known sequence number to replay missed messages.

---

## Key Components

### ChatPanel

The main interface. Renders streaming messages, handles user input (Enter to send, Escape to abort), and integrates voice input/output buttons.

### PermissionDialog

Modal overlay shown when the agent needs permission. Displays:
- Tool name and description
- Risk level badge (high=red, medium=yellow, low=green)
- Command or file path
- Allow/Deny buttons
- "Remember" checkbox with scope selector (tool/path/session)

### ToolCallCard

Expandable card showing tool execution:
- Tool name and arguments
- Execution duration
- Result content (with syntax highlighting for code)
- Diff view for file edit operations

### SettingsPage

Tabbed settings interface:
- **Model** — current model info and tier
- **Permissions** — mode selector (ModeSwitch component)
- **Memory** — memory statistics and management
- **MCP Servers** — server status and configuration
- **About** — version info

### VirtualMessageList

Performance-optimized message list that only renders visible messages. Used instead of rendering all messages in long conversations.

---

## Adding a New Component

1. Create `frontend/src/components/MyComponent.jsx`:

```jsx
import { useForge } from '../contexts/ForgeContext'

export default function MyComponent({ title }) {
  const { agentStatus } = useForge()

  return (
    <div className="p-4 bg-zinc-800 rounded-lg">
      <h2 className="text-lg font-semibold text-zinc-100">{title}</h2>
      <p className="text-zinc-400">Agent is: {agentStatus}</p>
    </div>
  )
}
```

2. Import and use it in `App.jsx` or the appropriate parent component.

3. Add a test in `frontend/src/components/__tests__/MyComponent.test.jsx`:

```jsx
import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import MyComponent from '../MyComponent'

describe('MyComponent', () => {
  it('renders the title', () => {
    render(<MyComponent title="Hello" />)
    expect(screen.getByText('Hello')).toBeDefined()
  })
})
```

---

## Keyboard Shortcuts

Managed by `useKeyboardShortcuts.js`. Current shortcuts:

| Shortcut | Action |
|----------|--------|
| Ctrl+/ | Toggle Settings |
| Ctrl+Shift+M | Toggle Memory Panel |
| Ctrl+Shift+D | Toggle Document Panel |
| Ctrl+Shift+E | Toggle Code Editor |
| Ctrl+Shift+V | Toggle Voice Input |
| Escape | Abort running query |

---

## Building and Running

### Development

```bash
cd rust/frontend
npm install
npx vite --port 3010
```

The Vite dev server proxies API requests to the backend:
- `/ws` -> `http://localhost:3011` (WebSocket)
- `/health`, `/readyz` -> `http://localhost:3011`
- `/api/*` -> `http://localhost:3011`

### Production Build

```bash
npm run build    # outputs to dist/
npm run preview  # preview the built app
```

The build uses code splitting for performance:
- `react-vendor` chunk (React, React DOM)
- `markdown` chunk (React Markdown, Remark)
- `syntax-highlighter` chunk (Prism languages)

### Testing

```bash
npm test          # single run
npm run test:watch # watch mode
```

See [Testing Guide](testing-guide.md) for writing frontend tests.

---

## Styling

- **Tailwind CSS** for all styling — no CSS modules or styled-components
- **Dark theme** — zinc color palette as the base
- **Font** — JetBrains Mono / Fira Code / Cascadia Code (monospace)
- **Custom CSS** in `index.css`:
  - Custom scrollbar styling
  - `.animate-pulse-dot` for thinking indicator animation

### Color Conventions

| Element | Colors |
|---------|--------|
| Background | `bg-zinc-900` (main), `bg-zinc-800` (panels) |
| Text | `text-zinc-100` (primary), `text-zinc-400` (secondary) |
| Accent | `text-blue-400`, `bg-blue-600` |
| Error | `text-red-400`, `bg-red-600` |
| Warning | `text-yellow-400` |
| Success | `text-green-400` |

---

## REST API Helper

`apiFetch.js` wraps `fetch()` with the auth header:

```javascript
import { apiFetch } from '../apiFetch'

// GET request
const sessions = await apiFetch('/api/sessions')

// POST request
const newMemory = await apiFetch('/api/memory', {
  method: 'POST',
  body: JSON.stringify({ type: 'user', title: 'Test', content: 'Content' })
})
```

The API key is read from localStorage on app load. See [API Reference](api-reference.md) for endpoint details.
