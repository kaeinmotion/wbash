use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Result, WErr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    ReadOnly,
    WriteWorkspace,
    Execute,
    GitWrite,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::ReadOnly => "read_only",
            Tier::WriteWorkspace => "write_workspace",
            Tier::Execute => "execute",
            Tier::GitWrite => "git_write",
        }
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Tier {
    type Err = WErr;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "read_only" => Ok(Tier::ReadOnly),
            "write_workspace" => Ok(Tier::WriteWorkspace),
            "execute" => Ok(Tier::Execute),
            "git_write" => Ok(Tier::GitWrite),
            other => Err(WErr::invalid_args(
                format!("unknown tier '{other}'"),
                "use read_only|write_workspace|execute|git_write",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Lean,
    Json,
}

impl Format {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "lean" => Ok(Format::Lean),
            "json" => Ok(Format::Json),
            other => Err(WErr::invalid_args(
                format!("unknown format '{other}'"),
                "use format=lean or format=json",
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exit {
    Code(i32),
    Timeout,
}

impl Exit {
    pub fn code(&self) -> Option<i32> {
        match self {
            Exit::Code(c) => Some(*c),
            Exit::Timeout => None,
        }
    }

    pub fn is_success(&self) -> bool {
        matches!(self, Exit::Code(0))
    }
}

impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exit::Code(c) => write!(f, "{c}"),
            Exit::Timeout => f.write_str("timeout"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Error => "error",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    pub severity: Severity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

impl Diagnostic {
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&self.file);
        s.push(':');
        s.push_str(&self.line.to_string());
        if let Some(c) = self.col {
            s.push(':');
            s.push_str(&c.to_string());
        }
        s.push(' ');
        s.push_str(self.severity.as_str());
        if let Some(code) = &self.code {
            s.push(' ');
            s.push_str(code);
        }
        s.push(' ');
        s.push_str(&self.message);
        s
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestStatus {
    Pass,
    Fail,
    Skip,
    Xfail,
    Error,
    Timeout,
}

impl TestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TestStatus::Pass => "pass",
            TestStatus::Fail => "fail",
            TestStatus::Skip => "skip",
            TestStatus::Xfail => "xfail",
            TestStatus::Error => "error",
            TestStatus::Timeout => "timeout",
        }
    }

    pub fn is_failure(self) -> bool {
        matches!(
            self,
            TestStatus::Fail | TestStatus::Error | TestStatus::Timeout
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub id: String,
    pub status: TestStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ItemClass {
    /// Never dropped by the budget fitter.
    MustKeep,
    High,
    Low,
}

impl ItemClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ItemClass::MustKeep => "must_keep",
            ItemClass::High => "high",
            ItemClass::Low => "low",
        }
    }
}

/// A rendered block of output. Budget fitting works at whole-item granularity;
/// elision never splits an item.
#[derive(Debug, Clone)]
pub struct Item {
    pub class: ItemClass,
    pub value: f32,
    pub lines: Vec<String>,
    pub est_tokens: u32,
}

impl Item {
    pub fn new(class: ItemClass, value: f32, lines: Vec<String>) -> Self {
        let est_tokens = crate::estimator::estimate_lines(&lines);
        Self {
            class,
            value,
            lines,
            est_tokens,
        }
    }

    pub fn line(class: ItemClass, value: f32, line: impl Into<String>) -> Self {
        Self::new(class, value, vec![line.into()])
    }

    pub fn must(line: impl Into<String>) -> Self {
        Self::new(ItemClass::MustKeep, 1000.0, vec![line.into()])
    }

    pub fn high(line: impl Into<String>) -> Self {
        Self::new(ItemClass::High, 10.0, vec![line.into()])
    }

    pub fn low(line: impl Into<String>) -> Self {
        Self::new(ItemClass::Low, 1.0, vec![line.into()])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Meta {
    #[serde(skip)]
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<Exit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub counts: BTreeMap<String, i64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub redactions: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Byte size of raw (pre-compaction) tool output, used for local savings stats.
    #[serde(skip)]
    pub raw_bytes: Option<u64>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleRef {
    pub id: String,
    pub kind: String,
    pub bytes: u64,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool: &'static str,
    pub header: String,
    pub items: Vec<Item>,
    pub diagnostics: Vec<Diagnostic>,
    pub handles: Vec<HandleRef>,
    pub meta: Meta,
    /// Optional raw/pre-compaction text for passthrough decisions (not serialized).
    pub raw_text: Option<String>,
}

impl ToolResult {
    pub fn new(tool: &'static str, header: impl Into<String>) -> Self {
        Self {
            tool,
            header: header.into(),
            items: Vec::new(),
            diagnostics: Vec::new(),
            handles: Vec::new(),
            meta: Meta {
                ok: true,
                ..Default::default()
            },
            raw_text: None,
        }
    }

    pub fn push(&mut self, item: Item) -> &mut Self {
        self.items.push(item);
        self
    }

    pub fn must(&mut self, line: impl Into<String>) -> &mut Self {
        self.push(Item::must(line))
    }

    pub fn high(&mut self, line: impl Into<String>) -> &mut Self {
        self.push(Item::high(line))
    }

    pub fn low(&mut self, line: impl Into<String>) -> &mut Self {
        self.push(Item::low(line))
    }

    pub fn note(&mut self, note: impl Into<String>) -> &mut Self {
        self.meta.notes.push(note.into());
        self
    }

    pub fn handle(&mut self, h: HandleRef) -> &mut Self {
        self.handles.push(h);
        self
    }

    pub fn diag(&mut self, d: Diagnostic) -> &mut Self {
        self.diagnostics.push(d);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Full,
    Lines { start: u32, end: u32 },
    Symbol,
    Outline,
}

impl Scope {
    pub fn key(self) -> String {
        match self {
            Scope::Full => "full".to_string(),
            Scope::Lines { start, end } => format!("lines:{start}:{end}"),
            Scope::Symbol => "symbol".to_string(),
            Scope::Outline => "outline".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadSeen {
    pub full_hash: String,
    pub short: String,
    pub mtime_ms: i64,
    pub size: u64,
    pub scope: String,
    pub snapshot_ref: Option<String>,
    pub shown_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSeen {
    pub snapshot_ref: Option<String>,
    pub files: usize,
    pub matches: usize,
    pub shown_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSeen {
    pub exit: Exit,
    pub results: BTreeMap<String, TestStatus>,
    pub shown_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    #[serde(default)]
    pub reads: BTreeMap<String, ReadSeen>,
    #[serde(default)]
    pub searches: BTreeMap<String, SearchSeen>,
    #[serde(default)]
    pub runs: BTreeMap<String, RunSeen>,
}

impl Ledger {
    pub fn read_entry(
        &self,
        path: &str,
        scope: Scope,
        ttl_secs: u64,
        now_ms: i64,
    ) -> Option<&ReadSeen> {
        let e = self.reads.get(&format!("{path}#{}", scope.key()))?;
        if now_ms - e.shown_ms > (ttl_secs as i64) * 1000 {
            return None;
        }
        Some(e)
    }

    pub fn record_read(&mut self, path: &str, scope: Scope, seen: ReadSeen) {
        self.reads.insert(format!("{path}#{}", scope.key()), seen);
    }

    pub fn invalidate_path(&mut self, path: &str) {
        let prefix = format!("{path}#");
        self.reads.retain(|k, _| !k.starts_with(&prefix));
    }

    pub fn touched(&self, path: &str) -> bool {
        let prefix = format!("{path}#");
        self.reads.keys().any(|k| k.starts_with(&prefix))
    }

    pub fn search_entry(&self, key: &str, ttl_secs: u64, now_ms: i64) -> Option<&SearchSeen> {
        let e = self.searches.get(key)?;
        if now_ms - e.shown_ms > (ttl_secs as i64) * 1000 {
            return None;
        }
        Some(e)
    }

    pub fn record_search(&mut self, key: String, seen: SearchSeen) {
        self.searches.insert(key, seen);
    }

    pub fn invalidate_searches(&mut self) {
        self.searches.clear();
    }

    pub fn expire(&mut self, ttl_secs: u64, now_ms: i64) {
        let ttl = (ttl_secs as i64) * 1000;
        self.reads.retain(|_, v| now_ms - v.shown_ms <= ttl);
        self.searches.retain(|_, v| now_ms - v.shown_ms <= ttl);
        self.runs.retain(|_, v| now_ms - v.shown_ms <= ttl);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleRecord {
    pub id: String,
    pub kind: String,
    pub bytes: u64,
    pub hash: String,
    pub created_ms: i64,
    pub expires_ms: i64,
    pub last_access_ms: i64,
    pub redacted: bool,
    pub content_type: String,
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn file_mtime_ms(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Duration wire formatting (spec §6.2): <10000ms => Nms else N.Ns.
pub fn format_duration_ms(ms: u64) -> String {
    if ms < 10_000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}
