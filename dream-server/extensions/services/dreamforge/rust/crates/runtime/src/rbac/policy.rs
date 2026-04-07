//! Policy engine: evaluates actions against RBAC rules.

use serde::{Deserialize, Serialize};

use super::roles::{action_to_permission, Role};

/// Decision from the RBAC engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RbacDecision {
    Allow,
    Deny,
    Ask,
}

/// A single policy rule (typically loaded from YAML).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    /// Tool name or `"*"` for all tools.
    pub tool: String,
    /// Access level: `"read"`, `"write"`, `"execute"`, or `null`.
    #[serde(default)]
    pub access_level: Option<String>,
    /// Decision: `"allow"`, `"deny"`, or `"ask"`.
    pub decision: String,
    /// Human-readable reason.
    #[serde(default)]
    pub reason: String,
    /// Priority (higher wins). Org rules should be 300+.
    #[serde(default)]
    pub priority: u32,
}

/// RBAC policy engine.
pub struct PolicyEngine {
    rules: Vec<PolicyRule>,
}

impl PolicyEngine {
    /// Create an engine with no rules (falls through to role-based defaults).
    #[must_use]
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Create an engine with a set of rules.
    #[must_use]
    pub fn with_rules(mut rules: Vec<PolicyRule>) -> Self {
        // Sort by priority descending so highest-priority rules match first
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        Self { rules }
    }

    /// Evaluate whether a role can perform an action on a tool.
    ///
    /// Decision priority:
    /// 1. Explicit DENY rules (veto power)
    /// 2. Explicit ALLOW rules
    /// 3. Fall back to role-based permissions
    #[must_use]
    pub fn evaluate(&self, role: Role, action: &str, tool_name: &str) -> RbacDecision {
        // Check rules in priority order
        let mut best_allow: Option<&PolicyRule> = None;

        for rule in &self.rules {
            if !rule_matches(rule, tool_name) {
                continue;
            }

            match rule.decision.as_str() {
                "deny" => return RbacDecision::Deny, // Immediate veto
                "allow" => {
                    if best_allow.is_none() {
                        best_allow = Some(rule);
                    }
                }
                "ask" => {
                    if best_allow.is_none() {
                        return RbacDecision::Ask;
                    }
                }
                _ => {}
            }
        }

        if best_allow.is_some() {
            return RbacDecision::Allow;
        }

        // Fall back to role-based permissions
        let required_perm = action_to_permission(action);
        if role.has_permission(required_perm) {
            RbacDecision::Allow
        } else {
            RbacDecision::Deny
        }
    }
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn rule_matches(rule: &PolicyRule, tool_name: &str) -> bool {
    rule.tool == "*" || rule.tool == tool_name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_rules_falls_through_to_role() {
        let engine = PolicyEngine::new();

        assert_eq!(
            engine.evaluate(Role::Developer, "tool.bash", "bash"),
            RbacDecision::Allow
        );
        assert_eq!(
            engine.evaluate(Role::Viewer, "tool.bash", "bash"),
            RbacDecision::Deny
        );
    }

    #[test]
    fn deny_rule_overrides_role() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule {
            tool: "bash".into(),
            access_level: Some("execute".into()),
            decision: "deny".into(),
            reason: "org policy".into(),
            priority: 300,
        }]);

        assert_eq!(
            engine.evaluate(Role::Admin, "tool.bash", "bash"),
            RbacDecision::Deny
        );
    }

    #[test]
    fn allow_rule_grants_access() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule {
            tool: "read_file".into(),
            access_level: None,
            decision: "allow".into(),
            reason: "safe tool".into(),
            priority: 100,
        }]);

        assert_eq!(
            engine.evaluate(Role::Viewer, "file.read", "read_file"),
            RbacDecision::Allow
        );
    }

    #[test]
    fn wildcard_rule_matches_all_tools() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule {
            tool: "*".into(),
            access_level: None,
            decision: "deny".into(),
            reason: "lockdown".into(),
            priority: 500,
        }]);

        assert_eq!(
            engine.evaluate(Role::Admin, "tool.anything", "anything"),
            RbacDecision::Deny
        );
    }

    #[test]
    fn higher_priority_deny_beats_lower_allow() {
        let engine = PolicyEngine::with_rules(vec![
            PolicyRule {
                tool: "bash".into(),
                access_level: None,
                decision: "allow".into(),
                reason: "project rule".into(),
                priority: 100,
            },
            PolicyRule {
                tool: "bash".into(),
                access_level: None,
                decision: "deny".into(),
                reason: "org override".into(),
                priority: 300,
            },
        ]);

        assert_eq!(
            engine.evaluate(Role::Developer, "tool.bash", "bash"),
            RbacDecision::Deny
        );
    }

    #[test]
    fn ask_decision_prompts_user() {
        let engine = PolicyEngine::with_rules(vec![PolicyRule {
            tool: "write_file".into(),
            access_level: None,
            decision: "ask".into(),
            reason: "needs review".into(),
            priority: 200,
        }]);

        assert_eq!(
            engine.evaluate(Role::Developer, "file.write", "write_file"),
            RbacDecision::Ask
        );
    }
}
