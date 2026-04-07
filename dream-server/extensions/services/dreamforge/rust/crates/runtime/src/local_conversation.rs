// DreamForge — local agentic coding assistant
// Copyright (c) 2026 Light-Heart-Labs. All rights reserved.
//! Local-first conversation runtime for DreamForge.
//! Wraps the core ConversationRuntime with features optimized for
//! local model inference: integrated loop detection, token budget
//! tracking, model tier awareness, and malformed JSON recovery.

use std::fmt::{Display, Formatter};
use std::time::Instant;

use crate::config::RuntimeFeatureConfig;
use crate::conversation::{
    ApiClient, ApiRequest, AssistantEvent, ConversationRuntime, RuntimeError, ToolError,
    ToolExecutor, TurnSummary,
};
use crate::hooks::HookAbortSignal;
use crate::permissions::{PermissionPolicy, PermissionPrompter};
use crate::session::Session;

// ---------------------------------------------------------------------------
// Loop detection
// ---------------------------------------------------------------------------

/// Configuration for the integrated loop detector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopDetectionConfig {
    /// Maximum consecutive calls to the exact same tool before aborting.
    pub max_consecutive_same_tool: u32,
    /// Maximum consecutive tool errors before aborting.
    pub max_consecutive_errors: u32,
    /// Maximum total tool calls in a single turn before aborting.
    pub max_total_tool_calls: u32,
}

impl Default for LoopDetectionConfig {
    fn default() -> Self {
        Self {
            max_consecutive_same_tool: 7,
            max_consecutive_errors: 5,
            max_total_tool_calls: 50,
        }
    }
}

/// Mutable state accumulated by the loop detector during a turn.
#[derive(Debug, Default)]
struct LoopDetectionState {
    last_tool_name: Option<String>,
    consecutive_same_tool: u32,
    consecutive_errors: u32,
    total_tool_calls: u32,
}

impl LoopDetectionState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Record a tool invocation. Returns `Some(reason)` if a loop is detected.
    fn record_call(
        &mut self,
        tool_name: &str,
        is_error: bool,
        config: &LoopDetectionConfig,
    ) -> Option<String> {
        self.total_tool_calls += 1;

        // --- consecutive same-tool check ---
        if self.last_tool_name.as_deref() == Some(tool_name) {
            self.consecutive_same_tool += 1;
        } else {
            self.consecutive_same_tool = 1;
            self.last_tool_name = Some(tool_name.to_owned());
        }

        // --- consecutive error check ---
        if is_error {
            self.consecutive_errors += 1;
        } else {
            self.consecutive_errors = 0;
        }

        // --- evaluate thresholds ---
        if self.consecutive_same_tool >= config.max_consecutive_same_tool {
            return Some(format!(
                "loop detected: tool `{}` called {} consecutive times (limit {})",
                tool_name, self.consecutive_same_tool, config.max_consecutive_same_tool,
            ));
        }
        if self.consecutive_errors >= config.max_consecutive_errors {
            return Some(format!(
                "loop detected: {} consecutive tool errors (limit {})",
                self.consecutive_errors, config.max_consecutive_errors,
            ));
        }
        if self.total_tool_calls >= config.max_total_tool_calls {
            return Some(format!(
                "loop detected: {} total tool calls (limit {})",
                self.total_tool_calls, config.max_total_tool_calls,
            ));
        }

        None
    }
}

/// A `ToolExecutor` wrapper that integrates loop detection.
pub struct LoopDetectingExecutor<T> {
    inner: T,
    state: LoopDetectionState,
    config: LoopDetectionConfig,
    /// Set to `Some(reason)` once a loop is detected.
    triggered: Option<String>,
}

impl<T: ToolExecutor> LoopDetectingExecutor<T> {
    pub fn new(inner: T, config: LoopDetectionConfig) -> Self {
        Self {
            inner,
            state: LoopDetectionState::default(),
            config,
            triggered: None,
        }
    }

    /// Returns `true` if the detector has fired.
    pub fn is_triggered(&self) -> bool {
        self.triggered.is_some()
    }

    /// Returns the detection reason, if any.
    pub fn trigger_reason(&self) -> Option<&str> {
        self.triggered.as_deref()
    }

    /// Reset detection state (e.g. between turns).
    pub fn reset(&mut self) {
        self.state.reset();
        self.triggered = None;
    }

    /// Total tool calls recorded so far.
    pub fn tool_call_count(&self) -> u32 {
        self.state.total_tool_calls
    }
}

impl<T: ToolExecutor> ToolExecutor for LoopDetectingExecutor<T> {
    fn execute(&mut self, tool_name: &str, input: &str) -> Result<String, ToolError> {
        // If already triggered, short-circuit.
        if let Some(reason) = &self.triggered {
            return Err(ToolError::new(format!("[loop-detector] {reason}")));
        }

        let result = self.inner.execute(tool_name, input);
        let is_error = result.is_err();

        if let Some(reason) = self.state.record_call(tool_name, is_error, &self.config) {
            self.triggered = Some(reason);
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Token budget tracking
// ---------------------------------------------------------------------------

/// Tracks estimated token consumption against the model's context window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenBudget {
    /// Total context window size advertised by the model.
    pub context_window: u32,
    /// Estimated tokens consumed by the system prompt (set once at turn start).
    pub system_prompt_tokens: u32,
    /// Tokens consumed so far during this turn (accumulated from usage events).
    pub used_tokens: u32,
    /// Tokens reserved for the model's response generation.
    pub reserve_tokens: u32,
}

impl TokenBudget {
    /// Default reserve kept for response generation.
    const DEFAULT_RESERVE: u32 = 4096;

    pub fn new(context_window: u32, system_prompt_tokens: u32) -> Self {
        Self {
            context_window,
            system_prompt_tokens,
            used_tokens: 0,
            reserve_tokens: Self::DEFAULT_RESERVE,
        }
    }

    /// Remaining tokens available for conversation content before the budget
    /// is considered exhausted.
    pub fn remaining(&self) -> u32 {
        self.context_window
            .saturating_sub(self.system_prompt_tokens)
            .saturating_sub(self.used_tokens)
            .saturating_sub(self.reserve_tokens)
    }

    /// Returns `true` when less than 10% of the usable budget remains.
    pub fn should_compact(&self) -> bool {
        let usable = self
            .context_window
            .saturating_sub(self.system_prompt_tokens)
            .saturating_sub(self.reserve_tokens);
        if usable == 0 {
            return true;
        }
        self.used_tokens >= (usable * 9) / 10
    }

    /// Returns `true` when the budget is completely exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.remaining() == 0
    }

    /// Record additional token usage.
    pub fn record_usage(&mut self, input_tokens: u32, _output_tokens: u32) {
        self.used_tokens = self.used_tokens.saturating_add(input_tokens);
    }
}

// ---------------------------------------------------------------------------
// Model tier awareness
// ---------------------------------------------------------------------------

/// Coarse capability tier inferred from model name and context size.
///
/// Used to select prompt verbosity and feature gating:
/// - `TierA`: large models (>30B params or >32K context) — full prompts
/// - `TierB`: medium models (7-30B or 16-32K context) — condensed prompts
/// - `TierC`: small models (<7B or <16K context) — minimal prompts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTier {
    TierA,
    TierB,
    TierC,
}

impl ModelTier {
    /// Heuristically detect model tier from a model name string and context
    /// window size.  This is intentionally coarse — the caller can always
    /// override the result.
    pub fn detect(model_name: &str, context_size: u32) -> Self {
        let lower = model_name.to_ascii_lowercase();

        // --- explicit large models ---
        if contains_param_size(&lower, &["70b", "72b", "65b", "405b"])
            || lower.contains("opus")
            || lower.contains("claude-3")
            || lower.contains("gpt-4")
            || lower.contains("command-r-plus")
            || lower.contains("mixtral-8x22b")
            || lower.contains("deepseek-v2")
        {
            return Self::TierA;
        }

        // --- medium models by name (check before small to avoid false negatives) ---
        if contains_param_size(&lower, &["7b", "8b", "13b", "14b", "22b"])
            || lower.contains("mistral")
            || lower.contains("codellama")
            || lower.contains("deepseek-coder")
        {
            return Self::TierB;
        }

        // --- explicit small models ---
        if contains_param_size(&lower, &["1b", "2b", "3b"])
            || lower.contains("phi-2")
            || lower.contains("phi-3-mini")
            || lower.contains("tinyllama")
        {
            return Self::TierC;
        }

        // --- fallback to context window size ---
        if context_size >= 32_768 {
            Self::TierA
        } else if context_size >= 16_384 {
            Self::TierB
        } else {
            Self::TierC
        }
    }

    /// Maximum recommended system prompt length (in estimated tokens) for
    /// this tier.
    pub fn max_system_prompt_tokens(&self) -> u32 {
        match self {
            Self::TierA => 8_192,
            Self::TierB => 3_072,
            Self::TierC => 1_024,
        }
    }
}

/// Check whether a lowercased model name contains a parameter-size token like
/// "7b", "13b", etc.  The match is boundary-aware: the character immediately
/// before the token (if any) must NOT be an ASCII digit, so that "3b" does not
/// spuriously match inside "13b".
fn contains_param_size(name: &str, sizes: &[&str]) -> bool {
    for size in sizes {
        if let Some(pos) = name.find(size) {
            if pos == 0 {
                return true;
            }
            let prev = name.as_bytes()[pos - 1];
            // Allow match only when preceded by a non-digit (e.g. '-', '.', '_').
            if !prev.is_ascii_digit() {
                return true;
            }
        }
    }
    false
}

impl Display for ModelTier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TierA => write!(f, "TierA (>30B / >32K)"),
            Self::TierB => write!(f, "TierB (7-30B / 16-32K)"),
            Self::TierC => write!(f, "TierC (<7B / <16K)"),
        }
    }
}

// ---------------------------------------------------------------------------
// Malformed JSON recovery
// ---------------------------------------------------------------------------

/// Attempt to recover a valid JSON value from a string that may contain
/// common formatting errors produced by local models.
///
/// Recovery strategies applied in order:
/// 1. Strip surrounding markdown code fences
/// 2. Fix trailing commas before `}` or `]`
/// 3. Fix unquoted keys (simple identifiers only)
///
/// Returns `Ok(value)` on success or the original `serde_json` error on
/// failure.
pub fn recover_json(raw: &str) -> Result<serde_json::Value, serde_json::Error> {
    // Fast path: valid JSON
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) {
        return Ok(value);
    }

    let mut text = raw.to_owned();

    // --- step 1: strip markdown code fences ---
    text = strip_code_fences(&text);

    // --- step 2: fix trailing commas ---
    text = fix_trailing_commas(&text);

    // --- step 3: fix unquoted keys ---
    text = fix_unquoted_keys(&text);

    serde_json::from_str::<serde_json::Value>(&text)
}

/// Strip leading/trailing markdown code fences (```json ... ``` or ``` ... ```).
fn strip_code_fences(input: &str) -> String {
    let trimmed = input.trim();
    let without_opening = if trimmed.starts_with("```json") {
        trimmed.strip_prefix("```json").unwrap_or(trimmed)
    } else if trimmed.starts_with("```") {
        trimmed.strip_prefix("```").unwrap_or(trimmed)
    } else {
        return trimmed.to_owned();
    };

    let without_closing = if let Some(pos) = without_opening.rfind("```") {
        &without_opening[..pos]
    } else {
        without_opening
    };

    without_closing.trim().to_owned()
}

/// Remove trailing commas before `}` or `]`.
fn fix_trailing_commas(input: &str) -> String {
    // Regex-free approach: iterate through characters.
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();

    let mut i = 0;
    while i < len {
        if chars[i] == ',' {
            // Look ahead past whitespace for `}` or `]`.
            let mut j = i + 1;
            while j < len && chars[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                // Skip the trailing comma.
                i += 1;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }

    result
}

/// Quote bare identifier keys that are not already quoted.
/// Only handles simple cases: `key:` -> `"key":`.
fn fix_unquoted_keys(input: &str) -> String {
    // Simple state machine: when we see an unquoted alphabetic sequence
    // followed by optional whitespace and a colon, wrap it in quotes.
    let mut result = String::with_capacity(input.len() + 32);
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut in_string = false;
    let mut escape_next = false;

    while i < len {
        if escape_next {
            result.push(chars[i]);
            escape_next = false;
            i += 1;
            continue;
        }

        if chars[i] == '\\' && in_string {
            result.push(chars[i]);
            escape_next = true;
            i += 1;
            continue;
        }

        if chars[i] == '"' {
            in_string = !in_string;
            result.push(chars[i]);
            i += 1;
            continue;
        }

        if in_string {
            result.push(chars[i]);
            i += 1;
            continue;
        }

        // Outside a string: look for bare identifier followed by `:`
        if chars[i].is_ascii_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let ident: String = chars[start..i].iter().collect();

            // Skip whitespace
            let mut j = i;
            while j < len && chars[j].is_ascii_whitespace() {
                j += 1;
            }

            if j < len && chars[j] == ':' {
                // This is a bare key — wrap in quotes.
                result.push('"');
                result.push_str(&ident);
                result.push('"');
            } else {
                // Not a key, just a bare word (shouldn't normally appear but
                // emit as-is for best-effort).
                result.push_str(&ident);
            }
            continue;
        }

        result.push(chars[i]);
        i += 1;
    }

    result
}

// ---------------------------------------------------------------------------
// JSON-recovering API client wrapper
// ---------------------------------------------------------------------------

/// Wraps an `ApiClient`, intercepting tool-use events to apply JSON recovery
/// on the tool input payload when it cannot be parsed directly.
pub struct RecoveringApiClient<C> {
    inner: C,
}

impl<C> RecoveringApiClient<C> {
    pub fn new(inner: C) -> Self {
        Self { inner }
    }
}

impl<C: ApiClient> ApiClient for RecoveringApiClient<C> {
    fn stream(&mut self, request: ApiRequest) -> Result<Vec<AssistantEvent>, RuntimeError> {
        let events = self.inner.stream(request)?;

        let recovered: Vec<AssistantEvent> = events
            .into_iter()
            .map(|event| match event {
                AssistantEvent::ToolUse { id, name, input } => {
                    let fixed_input = match recover_json(&input) {
                        Ok(value) => value.to_string(),
                        Err(_) => input, // pass through if unrecoverable
                    };
                    AssistantEvent::ToolUse {
                        id,
                        name,
                        input: fixed_input,
                    }
                }
                other => other,
            })
            .collect();

        Ok(recovered)
    }
}

// ---------------------------------------------------------------------------
// Terminal reason & local turn summary
// ---------------------------------------------------------------------------

/// Why a local turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalReason {
    /// The model finished normally (no more tool calls).
    Normal,
    /// The loop detector fired.
    LoopDetected,
    /// The token budget was exhausted before the turn could finish.
    TokenBudgetExhausted,
    /// The maximum iteration count was reached.
    MaxIterationsReached,
    /// The turn was explicitly aborted.
    Aborted,
    /// An error prevented completion.
    Error(String),
}

impl Display for TerminalReason {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::LoopDetected => write!(f, "loop detected"),
            Self::TokenBudgetExhausted => write!(f, "token budget exhausted"),
            Self::MaxIterationsReached => write!(f, "max iterations reached"),
            Self::Aborted => write!(f, "aborted"),
            Self::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}

/// Extended turn summary with DreamForge-specific telemetry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTurnSummary {
    /// The base summary from the wrapped `ConversationRuntime`.
    pub base: TurnSummary,
    /// Why the turn ended.
    pub terminal_reason: TerminalReason,
    /// Whether the loop detector fired during this turn.
    pub loop_detection_triggered: bool,
    /// Detected model capability tier.
    pub model_tier: ModelTier,
    /// Remaining token budget at turn end.
    pub token_budget_remaining: u32,
    /// Total tool calls executed during this turn.
    pub tool_call_count: u32,
    /// Wall-clock elapsed time in milliseconds.
    pub elapsed_ms: u64,
}

// ---------------------------------------------------------------------------
// LocalConversationRuntime
// ---------------------------------------------------------------------------

/// Local-first conversation runtime that wraps `ConversationRuntime` with
/// loop detection, token budget tracking, model tier awareness, and malformed
/// JSON recovery.
pub struct LocalConversationRuntime<C, T> {
    inner: ConversationRuntime<RecoveringApiClient<C>, LoopDetectingExecutor<T>>,
    model_tier: ModelTier,
    token_budget: TokenBudget,
    loop_config: LoopDetectionConfig,
}

impl<C, T> LocalConversationRuntime<C, T>
where
    C: ApiClient,
    T: ToolExecutor,
{
    /// Create a new local runtime wrapping the given API client and tool
    /// executor.
    pub fn new(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
        model_name: &str,
        context_window: u32,
    ) -> Self {
        let model_tier = ModelTier::detect(model_name, context_window);
        let system_prompt_tokens = estimate_prompt_tokens(&system_prompt);
        let token_budget = TokenBudget::new(context_window, system_prompt_tokens);
        let loop_config = LoopDetectionConfig::default();

        let recovering_client = RecoveringApiClient::new(api_client);
        let loop_executor = LoopDetectingExecutor::new(tool_executor, loop_config.clone());

        let inner = ConversationRuntime::new(
            session,
            recovering_client,
            loop_executor,
            permission_policy,
            system_prompt,
        );

        Self {
            inner,
            model_tier,
            token_budget,
            loop_config,
        }
    }

    /// Create a new local runtime with feature config (hooks, project settings).
    pub fn new_with_features(
        session: Session,
        api_client: C,
        tool_executor: T,
        permission_policy: PermissionPolicy,
        system_prompt: Vec<String>,
        model_name: &str,
        context_window: u32,
        feature_config: &RuntimeFeatureConfig,
    ) -> Self {
        let model_tier = ModelTier::detect(model_name, context_window);
        let system_prompt_tokens = estimate_prompt_tokens(&system_prompt);
        let token_budget = TokenBudget::new(context_window, system_prompt_tokens);
        let loop_config = LoopDetectionConfig::default();

        let recovering_client = RecoveringApiClient::new(api_client);
        let loop_executor = LoopDetectingExecutor::new(tool_executor, loop_config.clone());

        let inner = ConversationRuntime::new_with_features(
            session,
            recovering_client,
            loop_executor,
            permission_policy,
            system_prompt,
            feature_config,
        );

        Self {
            inner,
            model_tier,
            token_budget,
            loop_config,
        }
    }

    /// Override loop detection configuration.
    #[must_use]
    pub fn with_loop_detection(mut self, config: LoopDetectionConfig) -> Self {
        self.loop_config = config;
        self
    }

    /// Override the token budget reserve.
    #[must_use]
    pub fn with_reserve_tokens(mut self, reserve: u32) -> Self {
        self.token_budget.reserve_tokens = reserve;
        self
    }

    /// Override the maximum number of inner-loop iterations.
    #[must_use]
    pub fn with_max_iterations(self, max: usize) -> Self {
        Self {
            inner: self.inner.with_max_iterations(max),
            ..self
        }
    }

    /// Set auto-compaction threshold on the inner runtime.
    #[must_use]
    pub fn with_auto_compaction(self, threshold: u32) -> Self {
        Self {
            inner: self.inner.with_auto_compaction_input_tokens_threshold(threshold),
            ..self
        }
    }

    /// Set the abort signal on the inner runtime.
    #[must_use]
    pub fn with_abort_signal(self, signal: HookAbortSignal) -> Self {
        Self {
            inner: self.inner.with_hook_abort_signal(signal),
            ..self
        }
    }

    /// The detected model tier.
    pub fn model_tier(&self) -> ModelTier {
        self.model_tier
    }

    /// Current token budget snapshot.
    pub fn token_budget(&self) -> &TokenBudget {
        &self.token_budget
    }

    /// Run one turn of the conversation, producing a `LocalTurnSummary`.
    pub fn run_turn(
        &mut self,
        user_input: impl Into<String>,
        prompter: Option<&mut dyn PermissionPrompter>,
    ) -> Result<LocalTurnSummary, RuntimeError> {
        let start = Instant::now();

        // Pre-turn: check if budget is already exhausted.
        if self.token_budget.is_exhausted() {
            return Err(RuntimeError::new(
                "token budget exhausted before turn start",
            ));
        }

        // Delegate to the inner runtime.
        let result = self.inner.run_turn(user_input, prompter);
        let elapsed_ms = start.elapsed().as_millis() as u64;

        // Post-turn: update token budget from usage.
        let (base, terminal_reason) = match result {
            Ok(summary) => {
                self.token_budget
                    .record_usage(summary.usage.input_tokens, summary.usage.output_tokens);
                (summary, TerminalReason::Normal)
            }
            Err(err) => {
                let reason = classify_error(&err);
                // Return a minimal summary on error so the caller still gets
                // telemetry.
                let empty_summary = TurnSummary {
                    assistant_messages: Vec::new(),
                    tool_results: Vec::new(),
                    prompt_cache_events: Vec::new(),
                    iterations: 0,
                    usage: crate::usage::TokenUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        cache_creation_input_tokens: 0,
                        cache_read_input_tokens: 0,
                        local_inference_ms: None,
                    },
                    auto_compaction: None,
                };
                (empty_summary, reason)
            }
        };

        // Check if compaction is advisable.
        if self.token_budget.should_compact() && !self.token_budget.is_exhausted() {
            // Trigger compaction inside the inner runtime.
            let _ = self.inner.compact(crate::compact::CompactionConfig::default());
        }

        let loop_triggered = matches!(terminal_reason, TerminalReason::LoopDetected);

        Ok(LocalTurnSummary {
            tool_call_count: base.tool_results.len() as u32,
            base,
            terminal_reason,
            loop_detection_triggered: loop_triggered,
            model_tier: self.model_tier,
            token_budget_remaining: self.token_budget.remaining(),
            elapsed_ms,
        })
    }

    /// Access the underlying session.
    pub fn session(&self) -> &Session {
        self.inner.session()
    }
}

/// Rough token estimate: ~4 chars per token.
fn estimate_prompt_tokens(parts: &[String]) -> u32 {
    let total_chars: usize = parts.iter().map(|s| s.len()).sum();
    (total_chars / 4) as u32
}

/// Classify a `RuntimeError` into a `TerminalReason`.
fn classify_error(err: &RuntimeError) -> TerminalReason {
    let msg = err.to_string();
    if msg.contains("loop detected") {
        TerminalReason::LoopDetected
    } else if msg.contains("maximum number of iterations") {
        TerminalReason::MaxIterationsReached
    } else if msg.contains("token budget exhausted") {
        TerminalReason::TokenBudgetExhausted
    } else if msg.contains("aborted") {
        TerminalReason::Aborted
    } else {
        TerminalReason::Error(msg)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Loop detection tests ----

    #[test]
    fn loop_detection_triggers_on_consecutive_same_tool() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 3,
            max_consecutive_errors: 100,
            max_total_tool_calls: 100,
        };
        let mut state = LoopDetectionState::default();

        assert!(state.record_call("read_file", false, &config).is_none());
        assert!(state.record_call("read_file", false, &config).is_none());
        let reason = state.record_call("read_file", false, &config);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("read_file"));
    }

    #[test]
    fn loop_detection_resets_on_different_tool() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 3,
            max_consecutive_errors: 100,
            max_total_tool_calls: 100,
        };
        let mut state = LoopDetectionState::default();

        assert!(state.record_call("read_file", false, &config).is_none());
        assert!(state.record_call("read_file", false, &config).is_none());
        // Switch to a different tool — counter resets.
        assert!(state.record_call("write_file", false, &config).is_none());
        assert!(state.record_call("read_file", false, &config).is_none());
        assert!(state.record_call("read_file", false, &config).is_none());
        // Third consecutive read_file triggers.
        let reason = state.record_call("read_file", false, &config);
        assert!(reason.is_some());
    }

    #[test]
    fn loop_detection_triggers_on_consecutive_errors() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 100,
            max_consecutive_errors: 3,
            max_total_tool_calls: 100,
        };
        let mut state = LoopDetectionState::default();

        assert!(state.record_call("tool_a", true, &config).is_none());
        assert!(state.record_call("tool_b", true, &config).is_none());
        let reason = state.record_call("tool_c", true, &config);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("consecutive tool errors"));
    }

    #[test]
    fn loop_detection_error_counter_resets_on_success() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 100,
            max_consecutive_errors: 3,
            max_total_tool_calls: 100,
        };
        let mut state = LoopDetectionState::default();

        assert!(state.record_call("tool_a", true, &config).is_none());
        assert!(state.record_call("tool_b", true, &config).is_none());
        // Success resets the error counter.
        assert!(state.record_call("tool_c", false, &config).is_none());
        assert!(state.record_call("tool_d", true, &config).is_none());
        assert!(state.record_call("tool_e", true, &config).is_none());
        let reason = state.record_call("tool_f", true, &config);
        assert!(reason.is_some());
    }

    #[test]
    fn loop_detection_triggers_on_total_tool_calls() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 100,
            max_consecutive_errors: 100,
            max_total_tool_calls: 5,
        };
        let mut state = LoopDetectionState::default();

        for i in 0..4 {
            assert!(
                state
                    .record_call(&format!("tool_{i}"), false, &config)
                    .is_none()
            );
        }
        let reason = state.record_call("tool_4", false, &config);
        assert!(reason.is_some());
        assert!(reason.unwrap().contains("total tool calls"));
    }

    // ---- Model tier detection tests ----

    #[test]
    fn model_tier_detects_large_models() {
        assert_eq!(ModelTier::detect("llama-3.1-70b-instruct", 131_072), ModelTier::TierA);
        assert_eq!(ModelTier::detect("claude-3-opus", 200_000), ModelTier::TierA);
        assert_eq!(ModelTier::detect("gpt-4-turbo", 128_000), ModelTier::TierA);
        assert_eq!(ModelTier::detect("Qwen2.5-72B-Instruct", 32_768), ModelTier::TierA);
        assert_eq!(ModelTier::detect("deepseek-v2-chat", 128_000), ModelTier::TierA);
    }

    #[test]
    fn model_tier_detects_medium_models() {
        assert_eq!(ModelTier::detect("llama-3.1-8b-instruct", 131_072), ModelTier::TierB);
        assert_eq!(ModelTier::detect("codellama-13b", 16_384), ModelTier::TierB);
        assert_eq!(ModelTier::detect("mistral-7b-instruct", 32_768), ModelTier::TierB);
        assert_eq!(ModelTier::detect("deepseek-coder-6.7b", 16_384), ModelTier::TierB);
    }

    #[test]
    fn model_tier_detects_small_models() {
        assert_eq!(ModelTier::detect("phi-3-mini-4k", 4_096), ModelTier::TierC);
        assert_eq!(ModelTier::detect("tinyllama-1.1b", 2_048), ModelTier::TierC);
        assert_eq!(ModelTier::detect("gemma-2b-it", 8_192), ModelTier::TierC);
    }

    #[test]
    fn model_tier_falls_back_to_context_size() {
        assert_eq!(ModelTier::detect("some-unknown-model", 65_536), ModelTier::TierA);
        assert_eq!(ModelTier::detect("some-unknown-model", 16_384), ModelTier::TierB);
        assert_eq!(ModelTier::detect("some-unknown-model", 4_096), ModelTier::TierC);
    }

    // ---- JSON recovery tests ----

    #[test]
    fn json_recovery_valid_json_passes_through() {
        let input = r#"{"tool": "read_file", "path": "/tmp/test.rs"}"#;
        let value = recover_json(input).unwrap();
        assert_eq!(value["tool"], "read_file");
    }

    #[test]
    fn json_recovery_strips_code_fences() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let value = recover_json(input).unwrap();
        assert_eq!(value["key"], "value");
    }

    #[test]
    fn json_recovery_strips_plain_code_fences() {
        let input = "```\n{\"key\": 42}\n```";
        let value = recover_json(input).unwrap();
        assert_eq!(value["key"], 42);
    }

    #[test]
    fn json_recovery_fixes_trailing_commas() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let value = recover_json(input).unwrap();
        assert_eq!(value["a"], 1);
        assert_eq!(value["b"], 2);
    }

    #[test]
    fn json_recovery_fixes_trailing_comma_in_array() {
        let input = r#"[1, 2, 3,]"#;
        let value = recover_json(input).unwrap();
        assert_eq!(value.as_array().unwrap().len(), 3);
    }

    #[test]
    fn json_recovery_fixes_unquoted_keys() {
        let input = r#"{tool: "read_file", path: "/tmp/test.rs"}"#;
        let value = recover_json(input).unwrap();
        assert_eq!(value["tool"], "read_file");
        assert_eq!(value["path"], "/tmp/test.rs");
    }

    #[test]
    fn json_recovery_handles_combined_issues() {
        let input = "```json\n{tool: \"bash\", command: \"ls -la\",}\n```";
        let value = recover_json(input).unwrap();
        assert_eq!(value["tool"], "bash");
        assert_eq!(value["command"], "ls -la");
    }

    #[test]
    fn json_recovery_returns_error_on_unrecoverable() {
        let input = "this is not json at all {{{{";
        assert!(recover_json(input).is_err());
    }

    // ---- Token budget tests ----

    #[test]
    fn token_budget_remaining_calculation() {
        let budget = TokenBudget::new(32_768, 2_000);
        // remaining = 32768 - 2000 - 0 - 4096 = 26672
        assert_eq!(budget.remaining(), 26_672);
    }

    #[test]
    fn token_budget_exhaustion() {
        let mut budget = TokenBudget::new(8_192, 2_000);
        // usable = 8192 - 2000 - 4096 = 2096
        assert!(!budget.is_exhausted());
        budget.record_usage(2_096, 0);
        assert!(budget.is_exhausted());
    }

    #[test]
    fn token_budget_compaction_signal() {
        let mut budget = TokenBudget::new(8_192, 2_000);
        // usable = 8192 - 2000 - 4096 = 2096
        // 90% of 2096 = 1886.4 -> need used >= 1887
        assert!(!budget.should_compact());
        budget.record_usage(1_887, 0);
        assert!(budget.should_compact());
    }

    // ---- LoopDetectingExecutor integration ----

    struct EchoExecutor;

    impl ToolExecutor for EchoExecutor {
        fn execute(&mut self, _tool_name: &str, input: &str) -> Result<String, ToolError> {
            Ok(format!("echo: {input}"))
        }
    }

    #[test]
    fn loop_detecting_executor_blocks_after_trigger() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 2,
            max_consecutive_errors: 100,
            max_total_tool_calls: 100,
        };
        let mut executor = LoopDetectingExecutor::new(EchoExecutor, config);

        assert!(executor.execute("read_file", "{}").is_ok());
        assert!(executor.execute("read_file", "{}").is_ok());
        assert!(executor.is_triggered());
        // Subsequent calls should be blocked.
        let err = executor.execute("read_file", "{}").unwrap_err();
        assert!(err.to_string().contains("loop-detector"));
    }

    #[test]
    fn loop_detecting_executor_reset_clears_state() {
        let config = LoopDetectionConfig {
            max_consecutive_same_tool: 2,
            max_consecutive_errors: 100,
            max_total_tool_calls: 100,
        };
        let mut executor = LoopDetectingExecutor::new(EchoExecutor, config);

        assert!(executor.execute("read_file", "{}").is_ok());
        assert!(executor.execute("read_file", "{}").is_ok());
        assert!(executor.is_triggered());

        executor.reset();
        assert!(!executor.is_triggered());
        assert!(executor.execute("read_file", "{}").is_ok());
    }
}
