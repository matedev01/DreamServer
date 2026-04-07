//! DreamForge-specific configuration and prompt planning.
//!
//! Scans `DREAMFORGE_*` environment variables and builds tier-aware
//! system prompt sections with memory context injection.

use std::collections::HashMap;

/// DreamForge runtime configuration from environment variables.
#[derive(Debug, Clone)]
pub struct DreamForgeConfig {
    pub local_mode: bool,
    pub llm_api_url: String,
    pub model: String,
    pub workspace: String,
    pub permission_mode: String,
    pub max_turns: usize,
    pub rag_enabled: bool,
    pub rag_chunk_size: usize,
    pub rag_chunk_overlap: usize,
    pub memory_dir: String,
    pub extras: HashMap<String, String>,
}

impl DreamForgeConfig {
    /// Load from `DREAMFORGE_*` environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        let extras: HashMap<String, String> = std::env::vars()
            .filter(|(k, _)| k.starts_with("DREAMFORGE_"))
            .collect();

        Self {
            local_mode: env_bool("DREAMFORGE_LOCAL"),
            llm_api_url: env_or("LLM_API_URL", "http://localhost:11434"),
            model: env_or("DREAMFORGE_MODEL", ""),
            workspace: env_or("DREAMFORGE_WORKSPACE", "."),
            permission_mode: env_or("DREAMFORGE_PERMISSION_MODE", "default"),
            max_turns: env_or("DREAMFORGE_MAX_TURNS", "200")
                .parse()
                .unwrap_or(200),
            rag_enabled: env_bool("DREAMFORGE_RAG_ENABLED"),
            rag_chunk_size: env_or("DREAMFORGE_RAG_CHUNK_SIZE", "512")
                .parse()
                .unwrap_or(512),
            rag_chunk_overlap: env_or("DREAMFORGE_RAG_CHUNK_OVERLAP", "64")
                .parse()
                .unwrap_or(64),
            memory_dir: env_or("DREAMFORGE_MEMORY_DIR", ""),
            extras,
        }
    }
}

/// Tier-aware system prompt planning section.
#[derive(Debug, Clone)]
pub struct PromptPlan {
    pub tier_section: String,
    pub memory_section: String,
    pub tool_guidance: String,
}

/// Build a tier-aware prompt plan.
///
/// - Tier A models get full tool schemas
/// - Tier B models get schemas + validation reminders
/// - Tier C models get prompt-based fallback instructions
#[must_use]
pub fn build_prompt_plan(
    tier: &str,
    supports_tool_choice: bool,
    memory_context: &[String],
) -> PromptPlan {
    let tier_section = match tier {
        "A" => "You have excellent tool-calling capabilities. Use structured tool calls for all operations.".to_string(),
        "B" => concat!(
            "You have good tool-calling capabilities but may occasionally produce malformed calls. ",
            "Always validate your tool arguments match the schema before submitting. ",
            "If a tool call fails, check parameter types and retry."
        ).to_string(),
        _ => concat!(
            "Your tool-calling capabilities are limited. When you need to perform an action:\n",
            "1. State the tool name you want to use\n",
            "2. List the parameters as key: value pairs\n",
            "3. The system will execute the tool for you\n",
            "Do NOT attempt to format tool calls as JSON unless explicitly asked."
        ).to_string(),
    };

    let tool_guidance = if supports_tool_choice {
        String::new()
    } else {
        "Note: tool_choice is not supported. The model will decide when to call tools.".to_string()
    };

    let memory_section = if memory_context.is_empty() {
        String::new()
    } else {
        let mut section = String::from("## Relevant Context from Memory\n\n");
        for (i, entry) in memory_context.iter().enumerate() {
            section.push_str(&format!("{}. {}\n", i + 1, entry));
        }
        section
    };

    PromptPlan {
        tier_section,
        memory_section,
        tool_guidance,
    }
}

/// Assemble the full DreamForge system prompt addition.
#[must_use]
pub fn assemble_dreamforge_prompt(plan: &PromptPlan) -> String {
    let mut prompt = String::new();

    if !plan.tier_section.is_empty() {
        prompt.push_str("## Model Capabilities\n\n");
        prompt.push_str(&plan.tier_section);
        prompt.push_str("\n\n");
    }

    if !plan.tool_guidance.is_empty() {
        prompt.push_str(&plan.tool_guidance);
        prompt.push_str("\n\n");
    }

    if !plan.memory_section.is_empty() {
        prompt.push_str(&plan.memory_section);
        prompt.push('\n');
    }

    prompt
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_bool(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .is_some_and(|v| !v.is_empty() && v != "0" && v.to_ascii_lowercase() != "false")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_a_prompt_encourages_tool_calls() {
        let plan = build_prompt_plan("A", true, &[]);
        assert!(plan.tier_section.contains("excellent"));
        assert!(plan.tool_guidance.is_empty());
    }

    #[test]
    fn tier_b_prompt_adds_validation() {
        let plan = build_prompt_plan("B", true, &[]);
        assert!(plan.tier_section.contains("validate"));
    }

    #[test]
    fn tier_c_prompt_uses_fallback() {
        let plan = build_prompt_plan("C", false, &[]);
        assert!(plan.tier_section.contains("limited"));
        assert!(!plan.tool_guidance.is_empty());
    }

    #[test]
    fn memory_context_injected() {
        let memories = vec![
            "User prefers Rust over Python".to_string(),
            "Project uses Axum for HTTP".to_string(),
        ];
        let plan = build_prompt_plan("A", true, &memories);
        assert!(plan.memory_section.contains("Rust over Python"));
        assert!(plan.memory_section.contains("Axum"));
    }

    #[test]
    fn assemble_produces_complete_prompt() {
        let plan = build_prompt_plan("B", false, &["memory entry".to_string()]);
        let prompt = assemble_dreamforge_prompt(&plan);
        assert!(prompt.contains("Model Capabilities"));
        assert!(prompt.contains("memory entry"));
        assert!(prompt.contains("tool_choice is not supported"));
    }
}
