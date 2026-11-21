use regex::Regex;

/// Best-effort secret redaction applied before wire output, handle storage,
/// stats and audit records (spec §8.5).
pub struct Redactor {
    patterns: Vec<Regex>,
    enabled: bool,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    pub fn new() -> Self {
        let patterns = [
            // PEM private key blocks (dot matches newline via (?s)).
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            // AWS access key ids.
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            // GitHub / GitLab / Slack style tokens.
            r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_\-]{20,}|xox[abprs]-[A-Za-z0-9\-]{10,})\b",
            // key = value style assignments in env/config-like content.
            r#"(?i)\b(?:api[_-]?key|secret|token|password|passwd|pwd|client[_-]?secret)\b\s*[:=]\s*(?:"[^"\n]{4,}"|'[^'\n]{4,}'|[^\s'"\n]{4,})"#,
            // URL credentials.
            r"(?i)(?:https?|postgres(?:ql)?|mysql|mongodb|redis|amqps?)://[^\s/:@]+:[^\s/@]+@",
        ]
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect();
        Self {
            patterns,
            enabled: true,
        }
    }

    pub fn disabled() -> Self {
        Self {
            patterns: Vec::new(),
            enabled: false,
        }
    }

    /// Returns redacted text and the number of substitutions.
    /// Single-pass per pattern: one scan, one allocation only when a match is found.
    pub fn redact(&self, text: &str) -> (String, u32) {
        if !self.enabled {
            return (text.to_string(), 0);
        }
        let mut out = text.to_string();
        let mut total = 0u32;
        for re in &self.patterns {
            let mut count = 0u32;
            let replaced = re.replace_all(&out, |_caps: &regex::Captures| {
                count += 1;
                "[REDACTED]"
            });
            if count > 0 {
                out = replaced.into_owned();
                total += count;
            }
        }
        (out, total)
    }

    /// Secret-looking environment variable names are stripped from child envs.
    pub fn is_secret_key(key: &str) -> bool {
        let k = key.to_ascii_lowercase();
        k.contains("secret")
            || k.contains("token")
            || k.contains("password")
            || k.contains("passwd")
            || k.contains("api_key")
            || k.contains("apikey")
            || k.contains("credential")
            || k.contains("private_key")
    }
}
