//! Bridge between the async WebSocket layer and DreamForge's synchronous
//! `LocalConversationRuntime`.
//!
//! ## Key challenge
//!
//! `LocalConversationRuntime` uses a synchronous `PermissionPrompter` trait that
//! blocks the calling thread waiting for user input. In the server context,
//! permission decisions arrive asynchronously via WebSocket messages.
//!
//! ## Solution
//!
//! We use `tokio::sync::oneshot` channels: when a permission prompt fires,
//! the bridge stores the oneshot sender in a global map keyed by request_id,
//! sends a `PERMISSION_REQUEST` to the client, and blocks on the receiver.
//! When the client responds, `resolve_permission()` looks up the sender
//! and completes the round-trip.
//!
//! The entire `LocalConversationRuntime::run_turn` call happens inside
//! `tokio::task::spawn_blocking` since it contains synchronous blocking calls.
//!
//! Loop detection and malformed JSON recovery are handled internally by
//! `LocalConversationRuntime` — the bridge just configures it and processes
//! the extended `LocalTurnSummary` results.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tracing::{error, info, warn};

use crate::session_store::SessionStatus;
use crate::ws_types::{WsMessageType, WsOutgoing};
use crate::AppState;

// ---------- abort flag map ----------

type AbortMap = Mutex<HashMap<String, runtime::HookAbortSignal>>;

static ACTIVE_QUERIES: std::sync::OnceLock<AbortMap> = std::sync::OnceLock::new();

fn active_queries() -> &'static AbortMap {
    ACTIVE_QUERIES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Signal the running query for the given session to abort.
pub fn signal_abort(session_id: &str) {
    if let Some(signal) = active_queries()
        .lock()
        .expect("abort lock poisoned")
        .get(session_id)
    {
        signal.abort();
        info!(session_id, "abort signal sent to running query");
    }
}

// ---------- permission mode parsing ----------

/// Parse a human-readable permission mode label into the runtime enum.
fn parse_permission_mode(label: &str) -> runtime::PermissionMode {
    match label {
        "full_auto" | "dontAsk" | "danger-full-access" | "danger_full_access" => {
            runtime::PermissionMode::DangerFullAccess
        }
        "accept_edits" | "auto" | "workspace-write" | "workspace_write" => {
            runtime::PermissionMode::WorkspaceWrite
        }
        "plan" | "read-only" | "read_only" => runtime::PermissionMode::ReadOnly,
        _ => runtime::PermissionMode::Prompt, // "default" and anything else → prompt
    }
}

// ---------- tool definitions ----------

/// Tools that would deadlock, misbehave, or need subsystems not wired in the server.
const SERVER_BLOCKED_TOOLS: &[&str] = &[
    // Stdin-blocking / server-incompatible
    "AskUserQuestion",
    "EnterPlanMode",
    "ExitPlanMode",
    "TestingPermission",
    // Subprocess orchestration — needs server-side worker subsystem
    "Agent",
    "Skill",
    "ToolSearch",
    "StructuredOutput",
    "SendUserMessage",
    "RunTaskPacket",
    // Worker/Team — multi-agent coordination not wired
    "WorkerCreate",
    "WorkerGet",
    "WorkerObserve",
    "WorkerResolveTrust",
    "WorkerAwaitReady",
    "WorkerSendPrompt",
    "WorkerRestart",
    "WorkerTerminate",
    "TeamCreate",
    "TeamDelete",
    // LSP — language server not initialized in server context
    "LSP",
    // MCP OAuth — needs CLI-specific OAuth flow
    "McpAuth",
    // NotebookEdit — Jupyter not available in server context
    "NotebookEdit",
];

/// Build tool definitions from the full DreamForge `tools` crate, minus blocked ones,
/// plus any discovered MCP tools.
fn server_tool_definitions(
    mcp_tools: &[runtime::ManagedMcpTool],
) -> Vec<api::ToolDefinition> {
    let mut tools: Vec<api::ToolDefinition> = tools::mvp_tool_specs()
        .into_iter()
        .filter(|spec| !SERVER_BLOCKED_TOOLS.contains(&spec.name))
        .map(|spec| {
            // Ensure every schema has a "properties" field — some OpenAI-compat
            // APIs (LM Studio) reject function schemas without it.
            let mut schema = spec.input_schema;
            if let Some(obj) = schema.as_object_mut() {
                obj.entry("properties").or_insert_with(|| json!({}));
            }
            api::ToolDefinition {
                name: spec.name.to_string(),
                description: Some(spec.description.to_string()),
                input_schema: schema,
            }
        })
        .collect();

    // Add discovered MCP tools
    for mcp_tool in mcp_tools {
        let mut schema = mcp_tool
            .tool
            .input_schema
            .clone()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
        if let Some(obj) = schema.as_object_mut() {
            obj.entry("properties").or_insert_with(|| json!({}));
        }
        tools.push(api::ToolDefinition {
            name: mcp_tool.qualified_name.clone(),
            description: mcp_tool.tool.description.clone(),
            input_schema: schema,
        });
    }

    tools
}

// ---------- permission bridge ----------

type PermissionMap = Mutex<HashMap<String, oneshot::Sender<bool>>>;

static PENDING_PERMISSIONS: std::sync::OnceLock<PermissionMap> = std::sync::OnceLock::new();

fn pending_permissions() -> &'static PermissionMap {
    PENDING_PERMISSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Called from the WebSocket handler when a `PERMISSION_RESPONSE` arrives.
pub fn resolve_permission(request_id: &str, allow: bool) {
    if let Some(tx) = pending_permissions()
        .lock()
        .expect("permission lock poisoned")
        .remove(request_id)
    {
        let _ = tx.send(allow);
    }
}

/// A `PermissionPrompter` that sends permission requests over WebSocket
/// and blocks until the client responds via `resolve_permission`.
struct WsPermissionPrompter {
    ws_tx: mpsc::UnboundedSender<WsOutgoing>,
    session_id: String,
    seq: Arc<AtomicU64>,
}

impl runtime::PermissionPrompter for WsPermissionPrompter {
    fn decide(
        &mut self,
        request: &runtime::PermissionRequest,
    ) -> runtime::PermissionPromptDecision {
        let request_id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel::<bool>();

        pending_permissions()
            .lock()
            .expect("permission lock poisoned")
            .insert(request_id.clone(), tx);

        let _ = self.ws_tx.send(WsOutgoing::new(
            WsMessageType::PermissionRequest,
            json!({
                "request_id": request_id,
                "tool_name": request.tool_name,
                "input": request.input,
                "message": request.reason,
            }),
            Some(self.session_id.clone()),
            self.seq.fetch_add(1, Ordering::Relaxed),
        ));

        match rx.blocking_recv() {
            Ok(true) => runtime::PermissionPromptDecision::Allow,
            Ok(false) | Err(_) => runtime::PermissionPromptDecision::Deny {
                reason: "user denied permission via UI".to_string(),
            },
        }
    }
}

// ---------- ApiClient adapter ----------

/// Adapts `api::ProviderClient` (async) to runtime's synchronous `ApiClient` trait.
/// Owns a **dedicated** tokio runtime for blocking on async calls.
/// We cannot reuse the main runtime's handle because `block_on` from
/// `spawn_blocking` on the same runtime deadlocks.
struct ServerApiClient {
    rt: tokio::runtime::Runtime,
    client: api::ProviderClient,
    model: String,
    ws_tx: mpsc::UnboundedSender<WsOutgoing>,
    session_id: String,
    seq: Arc<AtomicU64>,
    mcp_tools: Vec<runtime::ManagedMcpTool>,
    abort_signal: runtime::HookAbortSignal,
}

impl runtime::ApiClient for ServerApiClient {
    fn stream(
        &mut self,
        request: runtime::ApiRequest,
    ) -> Result<Vec<runtime::AssistantEvent>, runtime::RuntimeError> {
        // Build the provider-level message request
        let messages: Vec<api::InputMessage> = request
            .messages
            .iter()
            .filter_map(|msg| conversation_message_to_input(msg))
            .collect();

        let system_prompt = if request.system_prompt.is_empty() {
            None
        } else {
            Some(request.system_prompt.join("\n\n"))
        };

        let tools = server_tool_definitions(&self.mcp_tools);
        let api_request = api::MessageRequest {
            model: self.model.clone(),
            max_tokens: 8192,
            messages,
            system: system_prompt,
            tools: Some(tools),
            tool_choice: Some(api::ToolChoice::Auto),
            stream: true,
        };

        // Block on the async stream call
        let result = self.rt.block_on(async {
            let mut stream = self
                .client
                .stream_message(&api_request)
                .await
                .map_err(|e| runtime::RuntimeError::new(e.to_string()))?;

            let mut events = Vec::new();
            // Track current tool call being streamed (id, name, accumulated JSON args)
            let mut pending_tool: Option<(String, String, String)> = None;

            while let Some(event) = stream
                .next_event()
                .await
                .map_err(|e| runtime::RuntimeError::new(e.to_string()))?
            {
                // Check abort between stream events for responsive cancellation
                if self.abort_signal.is_aborted() {
                    return Err(runtime::RuntimeError::new("query aborted by user"));
                }
                match event {
                    api::StreamEvent::ContentBlockDelta(delta) => {
                        match delta.delta {
                            api::ContentBlockDelta::TextDelta { text } => {
                                // Stream text delta to client in real-time
                                let _ = self.ws_tx.send(WsOutgoing::new(
                                    WsMessageType::AssistantText,
                                    serde_json::json!({"delta": &text}),
                                    Some(self.session_id.clone()),
                                    self.seq.fetch_add(1, Ordering::Relaxed),
                                ));
                                events.push(runtime::AssistantEvent::TextDelta(text));
                            }
                            api::ContentBlockDelta::InputJsonDelta { partial_json } => {
                                // Accumulate tool call arguments
                                if let Some((_, _, ref mut args)) = pending_tool {
                                    args.push_str(&partial_json);
                                }
                            }
                            _ => {}
                        }
                    }
                    api::StreamEvent::ContentBlockStart(start) => {
                        if let api::OutputContentBlock::ToolUse { id, name, .. } =
                            start.content_block
                        {
                            // Store the tool call info; we'll emit ToolUse when ContentBlockStop arrives
                            pending_tool = Some((id.clone(), name.clone(), String::new()));
                        }
                    }
                    api::StreamEvent::ContentBlockStop(_) => {
                        // Finalize any pending tool call
                        if let Some((id, name, args)) = pending_tool.take() {
                            info!("tool call: {} args={}", name, truncate_str(&args, 200));
                            events.push(runtime::AssistantEvent::ToolUse {
                                id,
                                name,
                                input: args,
                            });
                        }
                    }
                    api::StreamEvent::MessageDelta(delta) => {
                        let usage = delta.usage;
                        events.push(runtime::AssistantEvent::Usage(
                            runtime::TokenUsage {
                                input_tokens: usage.input_tokens,
                                output_tokens: usage.output_tokens,
                                cache_creation_input_tokens: usage.cache_creation_input_tokens,
                                cache_read_input_tokens: usage.cache_read_input_tokens,
                                local_inference_ms: None,
                            },
                        ));
                    }
                    api::StreamEvent::MessageStop(_) => {
                        events.push(runtime::AssistantEvent::MessageStop);
                    }
                    _ => {}
                }
            }

            Ok::<Vec<runtime::AssistantEvent>, runtime::RuntimeError>(events)
        })?;

        Ok(result)
    }
}

/// Convert a runtime `ConversationMessage` to an API `InputMessage`.
///
/// Maps runtime message roles + content blocks to the Anthropic-style
/// `InputMessage` format. The OpenAI-compat layer in `api` crate then
/// translates these to the OpenAI chat format (assistant tool_calls,
/// role=tool messages, etc.).
fn conversation_message_to_input(msg: &runtime::ConversationMessage) -> Option<api::InputMessage> {
    let role = match msg.role {
        runtime::MessageRole::User => "user",
        runtime::MessageRole::Assistant => "assistant",
        // Tool results are sent as "user" role with ToolResult content blocks.
        // The OpenAI translator converts these to role="tool" messages.
        runtime::MessageRole::Tool => "user",
        runtime::MessageRole::System => return None,
    };

    let content: Vec<api::InputContentBlock> = msg
        .blocks
        .iter()
        .filter_map(|block| match block {
            runtime::ContentBlock::Text { text } => {
                Some(api::InputContentBlock::Text { text: text.clone() })
            }
            runtime::ContentBlock::ToolUse { id, name, input } => {
                // Parse input string as JSON Value, falling back to string wrapper
                let input_value = serde_json::from_str(input)
                    .unwrap_or_else(|_| serde_json::json!({"input": input}));
                Some(api::InputContentBlock::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    input: input_value,
                })
            }
            runtime::ContentBlock::ToolResult {
                tool_use_id,
                output,
                is_error,
                ..
            } => Some(api::InputContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: vec![api::ToolResultContentBlock::Text {
                    text: output.clone(),
                }],
                is_error: *is_error,
            }),
        })
        .collect();

    if content.is_empty() {
        return None;
    }

    Some(api::InputMessage {
        role: role.to_string(),
        content,
    })
}

// ---------- ToolExecutor adapter ----------

/// Minimal tool executor that sends tool calls over WebSocket.
/// Real tool execution happens in the runtime crate's built-in tools.
///
/// Loop detection is handled by `LocalConversationRuntime`'s
/// `LoopDetectingExecutor` wrapper — this executor only handles dispatch
/// and WebSocket streaming.
struct ServerToolExecutor {
    ws_tx: mpsc::UnboundedSender<WsOutgoing>,
    session_id: String,
    seq: Arc<AtomicU64>,
    workspace: String,
    /// MCP server manager for routing MCP tool calls.
    mcp_manager: Option<Arc<Mutex<runtime::McpServerManager>>>,
    /// Dedicated tokio runtime for async MCP calls (same pattern as ServerApiClient).
    mcp_rt: Option<tokio::runtime::Runtime>,
}

impl runtime::ToolExecutor for ServerToolExecutor {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, runtime::ToolError> {
        // Send tool_call_start
        let _ = self.ws_tx.send(WsOutgoing::new(
            WsMessageType::ToolCallStart,
            json!({
                "tool_call_id": uuid::Uuid::new_v4().to_string(),
                "tool_name": tool_name,
                "arguments": input,
            }),
            Some(self.session_id.clone()),
            self.seq.fetch_add(1, Ordering::Relaxed),
        ));

        // Ensure tools resolve relative paths against the workspace
        let _ = std::env::set_current_dir(&self.workspace);

        // Parse input and delegate to the correct dispatcher
        let input_value: serde_json::Value = serde_json::from_str(input)
            .unwrap_or_else(|_| json!({"input": input}));

        let result = if tool_name.starts_with("mcp__") {
            self.call_mcp_tool(tool_name, &input_value)
        } else {
            tools::execute_tool(tool_name, &input_value)
                .map_err(|e| runtime::ToolError::new(e))
        };

        let (output, is_error) = match &result {
            Ok(out) => (out.clone(), false),
            Err(e) => {
                error!(tool = tool_name, err = %e, "tool execution failed");
                (e.to_string(), true)
            }
        };

        // Scan tool output for secrets before sending to LLM
        #[cfg(feature = "secret-scanner")]
        let output = {
            let redacted = runtime::secret_scanner::redact(&output);
            if redacted != output {
                warn!("secret scanner redacted secrets in {} output", tool_name);
            }
            redacted
        };

        // Send tool_call_result
        let _ = self.ws_tx.send(WsOutgoing::new(
            WsMessageType::ToolCallResult,
            json!({
                "tool_name": tool_name,
                "output": truncate_str(&output, 10_000),
                "is_error": is_error,
            }),
            Some(self.session_id.clone()),
            self.seq.fetch_add(1, Ordering::Relaxed),
        ));

        #[cfg(feature = "secret-scanner")]
        if !is_error {
            return Ok(output);
        }

        result
    }
}

impl ServerToolExecutor {
    /// Route a tool call to the MCP server manager.
    fn call_mcp_tool(
        &mut self,
        qualified_name: &str,
        args: &serde_json::Value,
    ) -> Result<String, runtime::ToolError> {
        let manager_arc = self
            .mcp_manager
            .as_ref()
            .ok_or_else(|| runtime::ToolError::new("MCP manager not initialized"))?;

        let rt = self
            .mcp_rt
            .as_ref()
            .ok_or_else(|| runtime::ToolError::new("MCP runtime not initialized"))?;

        let arguments = if args.is_null() || (args.is_object() && args.as_object().unwrap().is_empty()) {
            None
        } else {
            Some(args.clone())
        };

        let result = {
            let mut manager = manager_arc
                .lock()
                .map_err(|e| runtime::ToolError::new(format!("MCP lock poisoned: {e}")))?;
            rt.block_on(manager.call_tool(qualified_name, arguments))
        };

        match result {
            Ok(response) => {
                if let Some(result) = response.result {
                    // Extract text content from MCP tool result
                    let text_parts: Vec<String> = result
                        .content
                        .iter()
                        .filter_map(|c| {
                            if c.kind == "text" {
                                c.data.get("text").and_then(|v| v.as_str()).map(String::from)
                            } else {
                                Some(serde_json::to_string(&c.data).unwrap_or_default())
                            }
                        })
                        .collect();
                    if result.is_error == Some(true) {
                        Err(runtime::ToolError::new(text_parts.join("\n")))
                    } else {
                        Ok(text_parts.join("\n"))
                    }
                } else if let Some(error) = response.error {
                    Err(runtime::ToolError::new(format!(
                        "MCP error {}: {}",
                        error.code, error.message
                    )))
                } else {
                    Ok(String::new())
                }
            }
            Err(e) => Err(runtime::ToolError::new(format!("MCP call failed: {e}"))),
        }
    }
}

// ---------- query execution ----------

/// Run a user query through `LocalConversationRuntime` and stream results via WebSocket.
pub async fn run_query(
    state: Arc<AppState>,
    session_id: String,
    user_message: String,
    ws_tx: mpsc::UnboundedSender<WsOutgoing>,
    seq: Arc<AtomicU64>,
) {
    let tx = ws_tx.clone();
    let sid = session_id.clone();
    let seq_clone = Arc::clone(&seq);

    // Create abort signal and store in map so WS Abort handler can trigger it
    let abort_signal = runtime::HookAbortSignal::new();
    active_queries()
        .lock()
        .expect("abort lock poisoned")
        .insert(sid.clone(), abort_signal.clone());

    let abort_for_turn = abort_signal;
    // Build and run the LocalConversationRuntime on a blocking thread
    let result = tokio::task::spawn_blocking(move || {
        run_turn_blocking(
            &state,
            &session_id,
            &user_message,
            tx.clone(),
            seq_clone.clone(),
            abort_for_turn,
        )
    })
    .await;

    // Clean up abort flag
    active_queries()
        .lock()
        .expect("abort lock poisoned")
        .remove(&sid);

    match result {
        Ok(Ok(summary)) => {
            // Send compaction notice if compaction occurred
            if let Some(compaction) = &summary.base.auto_compaction {
                let _ = ws_tx.send(WsOutgoing::new(
                    WsMessageType::CompactionNotice,
                    json!({
                        "removed_message_count": compaction.removed_message_count,
                        "message": format!(
                            "Context compacted: {} older messages summarized to stay within context window.",
                            compaction.removed_message_count
                        ),
                    }),
                    Some(sid.clone()),
                    seq.fetch_add(1, Ordering::Relaxed),
                ));
            }

            // If loop detection triggered, send an error message to the client
            if summary.loop_detection_triggered {
                let _ = ws_tx.send(WsOutgoing::new(
                    WsMessageType::Error,
                    json!({
                        "message": format!(
                            "Agent loop detected: {}. Turn ended early.",
                            summary.terminal_reason
                        ),
                    }),
                    Some(sid.clone()),
                    seq.fetch_add(1, Ordering::Relaxed),
                ));
            }

            // Derive terminal condition string for the client
            let terminal_condition = match &summary.terminal_reason {
                runtime::TerminalReason::Normal => "normal",
                runtime::TerminalReason::LoopDetected => "loop_detected",
                runtime::TerminalReason::TokenBudgetExhausted => "budget_exhausted",
                runtime::TerminalReason::MaxIterationsReached => "max_iterations",
                runtime::TerminalReason::Aborted => "aborted",
                runtime::TerminalReason::Error(_) => "error",
            };

            let model_tier_str = match summary.model_tier {
                runtime::ModelTier::TierA => "tier_a",
                runtime::ModelTier::TierB => "tier_b",
                runtime::ModelTier::TierC => "tier_c",
            };

            // Send turn complete with cache token visibility
            let _ = ws_tx.send(WsOutgoing::new(
                WsMessageType::TurnComplete,
                json!({
                    "iterations": summary.base.iterations,
                    "tokens_in": summary.base.usage.input_tokens,
                    "tokens_out": summary.base.usage.output_tokens,
                    "cache_creation_tokens": summary.base.usage.cache_creation_input_tokens,
                    "cache_read_tokens": summary.base.usage.cache_read_input_tokens,
                    "tool_call_count": summary.tool_call_count,
                    "elapsed_ms": summary.elapsed_ms,
                }),
                Some(sid.clone()),
                seq.fetch_add(1, Ordering::Relaxed),
            ));

            let _ = ws_tx.send(WsOutgoing::new(
                WsMessageType::QueryComplete,
                json!({
                    "total_turns": summary.base.iterations,
                    "total_tokens_in": summary.base.usage.input_tokens,
                    "total_tokens_out": summary.base.usage.output_tokens,
                    "cache_creation_tokens": summary.base.usage.cache_creation_input_tokens,
                    "cache_read_tokens": summary.base.usage.cache_read_input_tokens,
                    "terminal_condition": terminal_condition,
                    "tool_call_count": summary.tool_call_count,
                    "elapsed_ms": summary.elapsed_ms,
                    "model_tier": model_tier_str,
                }),
                Some(sid),
                seq.fetch_add(1, Ordering::Relaxed),
            ));
        }
        Ok(Err(e)) => {
            error!("agent turn failed: {e}");
            let _ = ws_tx.send(WsOutgoing::error(
                &format!("agent error: {e}"),
                Some(sid),
                seq.fetch_add(1, Ordering::Relaxed),
            ));
        }
        Err(e) => {
            error!("spawn_blocking panicked: {e}");
            let _ = ws_tx.send(WsOutgoing::error(
                "internal error: agent task panicked",
                Some(sid),
                seq.fetch_add(1, Ordering::Relaxed),
            ));
        }
    }
}

/// Returns today's date as YYYY-MM-DD using a civil calendar conversion.
fn civil_date_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Days since Unix epoch
    let days = (secs / 86400) as i64;
    // Algorithm from Howard Hinnant's civil_from_days
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Tier A: full behavioral rules for large models with ample context.
const RULES_TIER_A: &str = r#"RULES:
1. Max 2 calls to any single tool per turn — then respond.
2. Use tool results immediately; never re-search for data you already have.
3. On tool error: try a different approach or respond. No same-tool retries.
4. Prefer 1-3 tool calls. Write code directly; avoid excessive planning.
5. Always end with a text response explaining what you did.
6. For code tasks: write_file then respond. Avoid repetitive TodoWrite calls.
7. When uncertain, respond with text — don't loop through tools."#;

/// Tier B: condensed rules for medium models.
const RULES_TIER_B: &str = r#"RULES:
1. Max 2 calls per tool, then respond.
2. Use results immediately — no redundant searches.
3. On error: different tool or respond. No retries.
4. 1-3 tool calls max. Write code directly.
5. Always finish with a text explanation."#;

/// Tier C: minimal instructions for small models.
const RULES_TIER_C: &str = "Use tools when needed. Always finish with a text response.";

/// Truncate each section in `prompt` that looks like a git diff/status block
/// to at most `max_lines` lines, appending a "[truncated]" marker.
fn truncate_git_sections(prompt: &mut [String], max_lines: usize) {
    for section in prompt.iter_mut() {
        let dominated_by_git = section.contains("git diff")
            || section.contains("git status")
            || section.starts_with("diff --git")
            || section.starts_with("On branch");
        if dominated_by_git {
            let lines: Vec<&str> = section.lines().collect();
            if lines.len() > max_lines {
                let mut truncated: String = lines[..max_lines].join("\n");
                truncated.push_str("\n[... truncated ...]");
                *section = truncated;
            }
        }
    }
}

/// Build the system prompt, scaled by `ModelTier`.
///
/// - **TierA** (>30B / >32K): full rich prompt, all rules, full memory, output style.
/// - **TierB** (7-30B / 16-32K): rich prompt with git sections truncated to 50 lines,
///   condensed rules, max 5 memories, no output style.
/// - **TierC** (<7B / <16K): simple fallback prompt, minimal rules, no memory, no context.
fn build_system_prompt(
    workspace: &str,
    model: &str,
    memory_store: &runtime::memory::MemoryStore,
    mcp_tools: &[runtime::ManagedMcpTool],
    tier: runtime::ModelTier,
) -> Vec<String> {
    // ---- TierC: minimal prompt, skip expensive discovery ----
    if tier == runtime::ModelTier::TierC {
        let tool_names: Vec<String> = server_tool_definitions(mcp_tools)
            .iter()
            .map(|t| t.name.clone())
            .collect();
        return vec![
            "You are DreamForge, a local coding assistant. Be concise.".to_string(),
            format!("Working directory: {workspace}"),
            format!("Tools: {}", tool_names.join(", ")),
            RULES_TIER_C.to_string(),
        ];
    }

    let today = civil_date_today();

    // Try the rich prompt builder (git status, file tree, CLAUDE.md, OS info)
    let mut prompt = match runtime::load_system_prompt(
        workspace,
        &today,
        std::env::consts::OS,
        "unknown",
    ) {
        Ok(sections) => {
            info!("loaded rich system prompt ({} sections)", sections.len());
            sections
        }
        Err(e) => {
            warn!("load_system_prompt failed ({}), using fallback", e);
            vec![
                "You are DreamForge, a local agentic coding assistant powered by the user's own GPU.".to_string(),
            ]
        }
    };

    // For TierB, truncate verbose git sections to keep prompt compact
    if tier == runtime::ModelTier::TierB {
        truncate_git_sections(&mut prompt, 50);
    }

    // Append DreamForge-specific context
    prompt.push(format!("Model: {model}"));
    prompt.push(format!("Working directory: {workspace}"));

    let tool_names: Vec<String> = server_tool_definitions(mcp_tools)
        .iter()
        .map(|t| t.name.clone())
        .collect();
    prompt.push(format!("You have access to {} tools: {}", tool_names.len(), tool_names.join(", ")));

    // Tier-appropriate behavioral rules
    match tier {
        runtime::ModelTier::TierA => prompt.push(RULES_TIER_A.to_string()),
        runtime::ModelTier::TierB => prompt.push(RULES_TIER_B.to_string()),
        runtime::ModelTier::TierC => unreachable!(), // handled above
    }

    // Output style injection — TierA only
    if tier == runtime::ModelTier::TierA {
        if let Ok(style) = std::env::var("DREAMFORGE_OUTPUT_STYLE") {
            prompt.push(format!("Output style: {style}"));
        }
    }

    // Memory injection — full for TierA, capped at 5 for TierB
    let memories = memory_store.load_all();
    if !memories.is_empty() {
        let max_memories = match tier {
            runtime::ModelTier::TierA => memories.len(),
            runtime::ModelTier::TierB => 5.min(memories.len()),
            runtime::ModelTier::TierC => 0,
        };
        if max_memories > 0 {
            let memory_section = memories[..max_memories]
                .iter()
                .map(|m| format!("- [{}] {}: {}", m.memory_type, m.title, m.content))
                .collect::<Vec<_>>()
                .join("\n");
            prompt.push(format!("## Memories\n{memory_section}"));
            info!("loaded {} memories into system prompt (tier {:?}, {} available)",
                  max_memories, tier, memories.len());
        }
    }

    prompt
}

/// Synchronous inner function that constructs and runs `LocalConversationRuntime`.
fn run_turn_blocking(
    state: &AppState,
    session_id: &str,
    user_input: &str,
    ws_tx: mpsc::UnboundedSender<WsOutgoing>,
    seq: Arc<AtomicU64>,
    abort_signal: runtime::HookAbortSignal,
) -> Result<runtime::LocalTurnSummary, String> {
    // Build the provider client
    let model = if state.config.model.is_empty() {
        "local".to_string()
    } else {
        state.config.model.clone()
    };

    // Set DREAMFORGE_LOCAL so provider detection routes to Local
    std::env::set_var("DREAMFORGE_LOCAL", "1");
    if !state.config.llm_api_url.is_empty() {
        std::env::set_var("LLM_API_URL", &state.config.llm_api_url);
    }

    let provider = api::ProviderClient::from_model(&model)
        .map_err(|e| format!("failed to create provider: {e}"))?;

    // Attach prompt cache (Anthropic-only; no-op for Local/OpenAi/Xai)
    let provider = provider.with_prompt_cache(api::PromptCache::new(session_id));

    // Apply retry policy for resilience (3 retries, 500ms→5s exponential backoff)
    let provider = match provider {
        api::ProviderClient::Local(client) => api::ProviderClient::Local(
            client.with_retry_policy(3, Duration::from_millis(500), Duration::from_secs(5)),
        ),
        api::ProviderClient::OpenAi(client) => api::ProviderClient::OpenAi(
            client.with_retry_policy(3, Duration::from_millis(500), Duration::from_secs(5)),
        ),
        api::ProviderClient::Xai(client) => api::ProviderClient::Xai(
            client.with_retry_policy(3, Duration::from_millis(500), Duration::from_secs(5)),
        ),
        // Anthropic provider: retry policy is applied when the feature is active.
        // Due to Cargo feature unification, the variant may be visible even when
        // this crate does not enable the feature, so we use a catch-all.
        #[allow(unreachable_patterns)]
        other => other,
    };

    let dedicated_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to create tokio runtime: {e}"))?;

    let api_client = ServerApiClient {
        rt: dedicated_rt,
        client: provider,
        model: model.clone(),
        ws_tx: ws_tx.clone(),
        session_id: session_id.to_string(),
        seq: Arc::clone(&seq),
        mcp_tools: state.mcp_tools.clone(),
        abort_signal: abort_signal.clone(),
    };

    // Build MCP runtime if manager is available
    let mcp_rt = if state.mcp_manager.is_some() {
        Some(
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to create MCP tokio runtime: {e}"))?,
        )
    } else {
        None
    };

    let tool_executor = ServerToolExecutor {
        ws_tx: ws_tx.clone(),
        session_id: session_id.to_string(),
        seq: Arc::clone(&seq),
        workspace: state.config.workspace.clone(),
        mcp_manager: state.mcp_manager.clone(),
        mcp_rt,
    };

    // Load conversation history from session store into the runtime session
    let mut session = runtime::Session::new();
    if let Some(stored) = state.sessions.get(session_id) {
        for msg_json in &stored.messages {
            let role = msg_json["role"].as_str().unwrap_or("");
            let content = msg_json["content"].as_str().unwrap_or("");
            if !content.is_empty() {
                match role {
                    "user" => session.messages.push(
                        runtime::ConversationMessage::user_text(content),
                    ),
                    "assistant" => session.messages.push(
                        runtime::ConversationMessage::assistant(
                            vec![runtime::ContentBlock::Text { text: content.to_string() }],
                        ),
                    ),
                    _ => {}
                }
            }
        }
        if !session.messages.is_empty() {
            info!(session_id, history_len = session.messages.len(), "loaded conversation history");
        }
    }
    let mode = parse_permission_mode(&state.config.permission_mode);
    info!("permission mode: {:?}", mode);
    let policy = runtime::PermissionPolicy::new(mode);

    // Load project config (CLAUDE.md, .forge/settings.json, hooks)
    let config_loader = runtime::ConfigLoader::default_for(&state.config.workspace);
    let runtime_config = config_loader.load().unwrap_or_else(|e| {
        warn!("failed to load runtime config: {e}, using defaults");
        runtime::RuntimeConfig::empty()
    });
    let feature_config = runtime_config.feature_config().clone();

    // Detect model tier before building the prompt so we can scale it
    let context_window = state.config.compact_threshold as u32 * 4;
    let detected_tier = runtime::ModelTier::detect(&model, context_window);

    // Build the tier-aware system prompt (git status, file tree, CLAUDE.md, memories)
    let system_prompt = build_system_prompt(
        &state.config.workspace,
        &model,
        &state.memory_store,
        &state.mcp_tools,
        detected_tier,
    );
    info!(model_tier = %detected_tier, prompt_sections = system_prompt.len(),
          "built tier-aware system prompt");

    // Build LocalConversationRuntime with integrated loop detection,
    // JSON recovery, token budget tracking, and model tier awareness.
    let mut local_runtime = runtime::LocalConversationRuntime::new_with_features(
        session,
        api_client,
        tool_executor,
        policy,
        system_prompt,
        &model,
        context_window,
        &feature_config,
    )
    .with_max_iterations(state.config.max_turns)
    .with_auto_compaction(state.config.compact_threshold)
    .with_abort_signal(abort_signal);

    let mut prompter = WsPermissionPrompter {
        ws_tx,
        session_id: session_id.to_string(),
        seq,
    };

    info!(session_id, model_tier = %local_runtime.model_tier(), "running agent turn");

    // Stream text deltas are sent by the ApiClient during stream processing.
    // Tool calls are sent by the ToolExecutor during execution.
    // Loop detection and JSON recovery are handled internally by LocalConversationRuntime.
    let summary = local_runtime
        .run_turn(user_input, Some(&mut prompter))
        .map_err(|e| e.to_string())?;

    // Send assistant text from the turn summary
    for msg in &summary.base.assistant_messages {
        for block in &msg.blocks {
            if let runtime::ContentBlock::Text { text } = block {
                let _ = state.sessions.get(session_id).map(|mut s| {
                    s.messages
                        .push(json!({"role": "assistant", "content": text}));
                    s.status = SessionStatus::Idle;
                    s.turn_count += 1;
                    s.total_tokens_in += u64::from(summary.base.usage.input_tokens);
                    s.total_tokens_out += u64::from(summary.base.usage.output_tokens);
                    state.sessions.put(s);
                });
            }
        }
    }

    // Persist session to disk after turn
    if let Some(sessions_dir) =
        crate::session_store::SessionStore::ensure_sessions_dir(&state.config.data_dir)
    {
        state.sessions.save_session_to_disk(session_id, &sessions_dir);
    }

    Ok(summary)
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_permission_completes_oneshot() {
        let (tx, rx) = oneshot::channel::<bool>();
        let request_id = "test-req-1".to_string();

        pending_permissions()
            .lock()
            .unwrap()
            .insert(request_id.clone(), tx);

        resolve_permission(&request_id, true);
        assert!(rx.blocking_recv().unwrap());
    }

    #[test]
    fn resolve_permission_noop_for_unknown_id() {
        resolve_permission("nonexistent", false);
    }

    #[test]
    fn truncate_handles_short_strings() {
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("hello world", 5), "hello");
    }
}
