/// Model tier detection for local LLMs.
///
/// Tiers classify models by their tool-calling reliability:
/// - **A**: 90%+ structured tool-call success (Qwen 2.5/3, Llama 3.x)
/// - **B**: 70-85% with validation required (Mistral, DeepSeek, Command-R)
/// - **C**: <60% success, uses prompt-based fallback (Phi, Gemma, unknown)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    A,
    B,
    C,
}

#[derive(Debug, Clone)]
pub struct TierInfo {
    pub tier: Tier,
    pub family: &'static str,
    pub chars_per_token: f32,
    pub supports_tool_choice: bool,
}

/// Known model families and their tier classification.
/// Prefixes are pre-normalized (lowercase, no hyphens/underscores/dots).
const TIER_TABLE: &[(&str, Tier, &str, f32)] = &[
    // Tier A — excellent tool calling
    ("qwen3", Tier::A, "qwen3", 3.5),
    ("qwen25coder", Tier::A, "qwen2.5-coder", 3.5),
    ("qwen25", Tier::A, "qwen2.5", 3.5),
    ("llama33", Tier::A, "llama-3.3", 4.2),
    ("llama32", Tier::A, "llama-3.2", 4.2),
    ("llama31", Tier::A, "llama-3.1", 4.2),
    // Tier B — moderate tool calling
    ("mistral", Tier::B, "mistral", 2.8),
    ("mixtral", Tier::B, "mixtral", 2.8),
    ("deepseek", Tier::B, "deepseek", 3.2),
    ("commandr", Tier::B, "command-r", 3.5),
    // Tier C — limited tool calling
    ("phi", Tier::C, "phi", 3.8),
    ("gemma", Tier::C, "gemma", 3.5),
    ("tinyllama", Tier::C, "tinyllama", 4.0),
];

/// Detect the tier for a model name.
///
/// Normalizes the name (lowercase, strip hyphens/underscores/dots) and matches
/// against known prefixes. Unknown models default to Tier C.
#[must_use]
pub fn detect_tier(model: &str) -> TierInfo {
    let normalized = normalize(model);

    for &(prefix, tier, family, cpt) in TIER_TABLE {
        if normalized.starts_with(prefix) {
            return TierInfo {
                tier,
                family,
                chars_per_token: cpt,
                supports_tool_choice: matches!(tier, Tier::A | Tier::B),
            };
        }
    }

    TierInfo {
        tier: Tier::C,
        family: "unknown",
        chars_per_token: 3.5,
        supports_tool_choice: false,
    }
}

fn normalize(model: &str) -> String {
    model
        .to_ascii_lowercase()
        .replace(['-', '_', '.'], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen3_is_tier_a() {
        let info = detect_tier("Qwen3-30B-A3B");
        assert_eq!(info.tier, Tier::A);
        assert_eq!(info.family, "qwen3");
        assert!(info.supports_tool_choice);
    }

    #[test]
    fn qwen25_coder_is_tier_a() {
        let info = detect_tier("qwen2.5-coder-32b-instruct");
        assert_eq!(info.tier, Tier::A);
        assert_eq!(info.family, "qwen2.5-coder");
    }

    #[test]
    fn llama_variants_are_tier_a() {
        for name in &["llama-3.3-70b", "llama3.2-3b", "Llama-3.1-8B"] {
            let info = detect_tier(name);
            assert_eq!(info.tier, Tier::A, "expected Tier A for {name}");
        }
    }

    #[test]
    fn mistral_is_tier_b() {
        let info = detect_tier("mistral-7b-instruct-v0.3");
        assert_eq!(info.tier, Tier::B);
        assert!(info.supports_tool_choice);
    }

    #[test]
    fn deepseek_is_tier_b() {
        let info = detect_tier("deepseek-coder-v2");
        assert_eq!(info.tier, Tier::B);
    }

    #[test]
    fn phi_is_tier_c() {
        let info = detect_tier("phi-3.5-mini");
        assert_eq!(info.tier, Tier::C);
        assert!(!info.supports_tool_choice);
    }

    #[test]
    fn unknown_model_defaults_to_tier_c() {
        let info = detect_tier("my-custom-model-7b");
        assert_eq!(info.tier, Tier::C);
        assert_eq!(info.family, "unknown");
    }
}
