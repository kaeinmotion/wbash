use std::fmt;

/// Stable error taxonomy (spec §8). Every error carries a stable code, a
/// one-line cause and (when correctable) a concrete hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WErr {
    pub code: &'static str,
    pub message: String,
    pub hint: Option<String>,
}

pub type Result<T> = std::result::Result<T, WErr>;

impl WErr {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_hint(
        code: &'static str,
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self::new(code, message).hint(hint)
    }

    pub fn invalid_args(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::with_hint("invalid_args", message, hint)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message)
    }

    /// CLI exit code for an error: invocation/policy/config errors are 2,
    /// internal errors are 70 (spec §4.1).
    pub fn cli_exit(&self) -> i32 {
        if self.code == "internal" {
            70
        } else {
            2
        }
    }

    pub fn render(&self) -> String {
        let msg = self.message.trim_end_matches('.');
        match &self.hint {
            Some(h) if !h.trim().is_empty() => format!("!{} {}. {}", self.code, msg, h.trim()),
            _ => format!("!{} {}", self.code, msg),
        }
    }
}

impl fmt::Display for WErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(h) = &self.hint {
            write!(f, " ({h})")?;
        }
        Ok(())
    }
}

impl std::error::Error for WErr {}

impl From<std::io::Error> for WErr {
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => "not_found",
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            _ => "internal",
        };
        WErr::new(code, e.to_string())
    }
}

impl From<serde_json::Error> for WErr {
    fn from(e: serde_json::Error) -> Self {
        WErr::new("internal", format!("json: {e}"))
    }
}
