use regex::{Regex, RegexSet};

/// Best-effort secret redaction applied before wire output, handle storage,
/// stats and audit records (spec §8.5).
pub struct Redactor {
    set: RegexSet,
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
        let raw = [
            // PEM private key blocks (dot matches newline via (?s)).
            r"(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            // AWS access key ids.
            r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b",
            // GitHub / GitLab / Slack style tokens.
            r"\b(?:gh[pousr]_[A-Za-z0-9]{20,}|glpat-[A-Za-z0-9_\-]{20,}|xox[abprs]-[A-Za-z0-9\-]{10,})\b",
            // Modern AI provider keys: OpenAI, Anthropic, HuggingFace, Stripe.
            r"\b(?:sk-(?:proj-)?[A-Za-z0-9_\-]{20,}|sk-ant-[A-Za-z0-9_\-]{20,}|hf_[A-Za-z0-9]{30,}|(?:sk|rk)_(?:live|test)_[0-9a-zA-Z]{24,})\b",
            // Google API keys (AIza...).
            r"\bAIza[0-9A-Za-z\-_]{30,40}\b",
            // JWT tokens (3 base64 segments).
            r"\beyJ[A-Za-z0-9-_]{10,}\.eyJ[A-Za-z0-9-_]{10,}\.[A-Za-z0-9-_]{10,}\b",
            // key = value style assignments in env/config-like content.
            r#"(?i)\b(?:api[_-]?key|secret|token|password|passwd|pwd|client[_-]?secret)\b\s*[:=]\s*(?:"[^"\n]{4,}"|'[^'\n]{4,}'|[^\s'"\n]{4,})"#,
            // URL credentials.
            r"(?i)(?:https?|postgres(?:ql)?|mysql|mongodb|redis|amqps?)://[^\s/:@]+:[^\s/@]+@",
        ];
        let set = RegexSet::new(raw).unwrap_or_else(|_| RegexSet::empty());
        let patterns = raw.iter().filter_map(|p| Regex::new(p).ok()).collect();
        Self {
            set,
            patterns,
            enabled: true,
        }
    }

    pub fn disabled() -> Self {
        Self {
            set: RegexSet::empty(),
            patterns: Vec::new(),
            enabled: false,
        }
    }

    /// Returns redacted text and the number of substitutions.
    /// Fast-path: single parallel DFA scan over text via RegexSet.
    /// If no patterns match, returns in O(L) without running individual regexes.
    pub fn redact(&self, text: &str) -> (String, u32) {
        if !self.enabled || self.patterns.is_empty() {
            return (text.to_string(), 0);
        }
        let matches = self.set.matches(text);
        if !matches.matched_any() {
            return (text.to_string(), 0);
        }
        let mut out = std::borrow::Cow::Borrowed(text);
        let mut total = 0u32;
        for idx in matches.into_iter() {
            let re = &self.patterns[idx];
            let mut count = 0u32;
            let replaced = re.replace_all(&out, |_caps: &regex::Captures| {
                count += 1;
                "[REDACTED]"
            });
            if count > 0 {
                out = std::borrow::Cow::Owned(replaced.into_owned());
                total += count;
            }
        }
        (out.into_owned(), total)
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
            || k.contains("auth")
            || k.contains("bearer")
    }
}
