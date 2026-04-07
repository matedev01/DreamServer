//! Scans text for secrets (API keys, tokens, private keys, etc.) and redacts them.
//!
//! Feature-gated behind `secret-scanner`. Hook into tool result paths to prevent
//! accidental credential leakage to the LLM context.

use std::sync::OnceLock;

use regex::Regex;

/// A compiled secret detection pattern.
struct SecretPattern {
    name: &'static str,
    regex: Regex,
}

/// Result of scanning text for secrets.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub has_secrets: bool,
    pub matches: Vec<SecretMatch>,
}

/// A single secret match with location info.
#[derive(Debug, Clone)]
pub struct SecretMatch {
    pub pattern_name: &'static str,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

// ---------- pattern table ----------

const PATTERN_DEFS: &[(&str, &str)] = &[
    // Cloud providers
    ("aws_access_key", r"AKIA[0-9A-Z]{16}"),
    ("aws_secret_key", r"(?i)aws_secret_access_key\s*[=:]\s*\S{20,}"),
    ("gcp_service_account", r#""type"\s*:\s*"service_account""#),
    // Code hosting
    ("github_token", r"gh[pousr]_[A-Za-z0-9_]{36,}"),
    ("gitlab_token", r"glpat-[A-Za-z0-9\-]{20,}"),
    ("bitbucket_app_password", r"ATBB[A-Za-z0-9]{32,}"),
    // AI providers
    ("anthropic_api_key", r"sk-ant-[A-Za-z0-9\-_]{20,}"),
    ("openai_api_key", r"sk-[A-Za-z0-9]{20,}"),
    // Communication
    ("slack_token", r"xox[bprs]-[A-Za-z0-9\-]{10,}"),
    (
        "discord_bot_token",
        r"[MN][A-Za-z0-9]{23,}\.[A-Za-z0-9_\-]{6}\.[A-Za-z0-9_\-]{27,}",
    ),
    // Payment/SaaS
    ("stripe_key", r"[sr]k_(live|test)_[A-Za-z0-9]{20,}"),
    (
        "twilio_auth",
        r"(?i)twilio[_\s]*auth[_\s]*token\s*[=:]\s*[a-f0-9]{32}",
    ),
    // Infrastructure
    ("npm_token", r"npm_[A-Za-z0-9]{36,}"),
    ("digitalocean_token", r"dop_v1_[A-Fa-f0-9]{64}"),
    (
        "sentry_dsn",
        r"https://[a-f0-9]{32}@[^\s/]+\.ingest\.sentry\.io",
    ),
    // Cryptographic material
    (
        "private_key",
        r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
    ),
    ("pgp_private_key", r"-----BEGIN PGP PRIVATE KEY BLOCK-----"),
    // Generic
    ("bearer_token", r"(?i)bearer\s+[A-Za-z0-9\-._~+/]{20,}"),
    ("basic_auth", r"(?i)basic\s+[A-Za-z0-9+/=]{20,}"),
    (
        "database_url",
        r"(?i)(?:postgres|mysql|mongodb|redis)://[^\s]{10,}",
    ),
    (
        "password_assignment",
        r#"(?i)(?:password|passwd|secret)\s*[=:]\s*["'][^\s"']{8,}["']"#,
    ),
    (
        "api_key_assignment",
        r#"(?i)(?:api[_\-]?key|api[_\-]?secret|auth[_\-]?token)\s*[=:]\s*["'][^\s"']{8,}["']"#,
    ),
    (
        "jwt_token",
        r"eyJ[A-Za-z0-9_\-]{10,}\.eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}",
    ),
];

fn compiled_patterns() -> &'static [SecretPattern] {
    static PATTERNS: OnceLock<Vec<SecretPattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        PATTERN_DEFS
            .iter()
            .map(|(name, pat)| SecretPattern {
                name,
                regex: Regex::new(pat).expect("invalid secret pattern regex"),
            })
            .collect()
    })
}

// ---------- public API ----------

/// Returns `true` if `text` contains any recognizable secrets.
#[must_use]
pub fn contains_secrets(text: &str) -> bool {
    compiled_patterns()
        .iter()
        .any(|p| p.regex.is_match(text))
}

/// Scan `text` and return all secret matches with location info.
#[must_use]
pub fn scan(text: &str) -> ScanResult {
    let patterns = compiled_patterns();
    let mut matches = Vec::new();

    for (line_idx, line) in text.lines().enumerate() {
        for pattern in patterns {
            for m in pattern.regex.find_iter(line) {
                matches.push(SecretMatch {
                    pattern_name: pattern.name,
                    line: line_idx + 1,
                    start: m.start(),
                    end: m.end(),
                });
            }
        }
    }

    ScanResult {
        has_secrets: !matches.is_empty(),
        matches,
    }
}

/// Replace all detected secrets in `text` with `[REDACTED:{pattern_name}]`.
#[must_use]
pub fn redact(text: &str) -> String {
    let patterns = compiled_patterns();
    let mut result = text.to_string();

    // Apply all patterns. Because replacements can change offsets, we use
    // regex replace_all which handles this correctly per-pattern.
    for pattern in patterns {
        let replacement = format!("[REDACTED:{}]", pattern.name);
        result = pattern
            .regex
            .replace_all(&result, replacement.as_str())
            .into_owned();
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_access_key() {
        let text = "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        assert!(contains_secrets(text));
        let result = scan(text);
        assert!(result.has_secrets);
        assert_eq!(result.matches[0].pattern_name, "aws_access_key");
    }

    #[test]
    fn detects_github_token() {
        let text = "GITHUB_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        assert!(contains_secrets(text));
    }

    #[test]
    fn detects_anthropic_key() {
        let text = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz";
        assert!(contains_secrets(text));
        let result = scan(text);
        assert_eq!(result.matches[0].pattern_name, "anthropic_api_key");
    }

    #[test]
    fn detects_private_key_header() {
        let text = "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAK...";
        assert!(contains_secrets(text));
    }

    #[test]
    fn detects_database_url() {
        let text = "DATABASE_URL=postgres://user:pass@localhost:5432/mydb";
        assert!(contains_secrets(text));
    }

    #[test]
    fn detects_jwt_token() {
        let text = "token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456ghi789";
        assert!(contains_secrets(text));
    }

    #[test]
    fn redacts_secrets_in_place() {
        let text = "key = AKIAIOSFODNN7EXAMPLE rest of line";
        let redacted = redact(text);
        assert!(redacted.contains("[REDACTED:aws_access_key]"));
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(redacted.contains("rest of line"));
    }

    #[test]
    fn no_false_positive_on_clean_text() {
        let text = "This is a normal log line with no secrets at all.";
        assert!(!contains_secrets(text));
        let result = scan(text);
        assert!(!result.has_secrets);
        assert!(result.matches.is_empty());
    }

    #[test]
    fn multi_line_scan_reports_correct_line_numbers() {
        let text = "line one\nAKIAIOSFODNN7EXAMPLE\nline three";
        let result = scan(text);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].line, 2);
    }

    #[test]
    fn redacts_multiple_patterns() {
        let text = "aws=AKIAIOSFODNN7EXAMPLE token=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijkl";
        let redacted = redact(text);
        assert!(redacted.contains("[REDACTED:aws_access_key]"));
        assert!(redacted.contains("[REDACTED:github_token]"));
    }
}
