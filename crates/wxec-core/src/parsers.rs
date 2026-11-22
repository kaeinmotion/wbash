use crate::types::{Diagnostic, Exit, TestResult};

/// Aggregated test counts reported by a test runner summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunCounts {
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
}

/// Result of normalizing a tool's captured output (spec §4.8).
#[derive(Debug, Clone, Default)]
pub struct ParsedRun {
    pub summary: Option<String>,
    pub tests: Vec<TestResult>,
    pub diagnostics: Vec<Diagnostic>,
    pub parser: Option<String>,
    /// Totals reported by runner summary lines. More authoritative than
    /// `tests` when a runner omits per-test records (for example `cargo test -q`).
    pub totals: Option<RunCounts>,
}

impl ParsedRun {
    pub fn failed_tests(&self) -> impl Iterator<Item = &TestResult> {
        self.tests.iter().filter(|t| t.status.is_failure())
    }

    pub fn counts(&self) -> (usize, usize, usize) {
        let passed = self
            .tests
            .iter()
            .filter(|t| matches!(t.status, crate::types::TestStatus::Pass))
            .count();
        let failed = self.tests.iter().filter(|t| t.status.is_failure()).count();
        let skipped = self.tests.len() - passed - failed;
        (passed, failed, skipped)
    }

    /// Summary totals when available, otherwise counts derived from per-test records.
    pub fn total_counts(&self) -> (usize, usize, usize) {
        match self.totals {
            Some(t) => (t.passed, t.failed, t.ignored),
            None => self.counts(),
        }
    }
}

/// Adapter for the parser registry (implemented by `wxec-parsers`).
pub trait ParserSet: Send + Sync {
    /// Return argv with semantics-preserving structured-output flags appended
    /// when a parser matches. Must be deterministic.
    fn inject_args(&self, argv: &[String]) -> Vec<String>;

    /// Normalize captured output; `None` means "no parser matched or parse failed".
    fn parse(&self, argv: &[String], exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun>;
}

/// Used when no parser set is configured.
pub struct NoParsers;

impl ParserSet for NoParsers {
    fn inject_args(&self, argv: &[String]) -> Vec<String> {
        argv.to_vec()
    }

    fn parse(
        &self,
        _argv: &[String],
        _exit: &Exit,
        _stdout: &str,
        _stderr: &str,
    ) -> Option<ParsedRun> {
        None
    }
}
