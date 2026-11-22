use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::paths;
use crate::types::{now_ms, ToolResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsEntry {
    pub ts_ms: i64,
    pub tool: String,
    pub ok: bool,
    pub out_tokens: u32,
    pub raw_tokens: u32,
    pub sh: bool,
    pub fallback: bool,
}

pub fn stats_dir() -> PathBuf {
    paths::cache_dir().join("stats")
}

/// Civil date from unix days (Howard Hinnant's algorithm) for YYYY-MM-DD names.
pub fn ymd_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    if m <= 2 {
        y += 1;
    }
    (y, m, d)
}

pub fn day_file(ts_ms: i64) -> PathBuf {
    let days = ts_ms.div_euclid(86_400_000);
    let (y, m, d) = ymd_from_days(days);
    stats_dir().join(format!("{y:04}-{m:02}-{d:02}.jsonl"))
}

/// Runs `f` under an exclusive, non-blocking file lock on `.stats.lock`.
/// Stats writes are fire-and-forget, so dropping a write on lock contention or failure
/// is acceptable and avoids uncoordinated writes or file corruption.
fn with_stats_lock<T: Default>(f: impl FnOnce() -> T) -> T {
    let dir = stats_dir();
    if paths::ensure_private_dir(&dir).is_err() {
        return T::default();
    }
    let lock_path = dir.join(".stats.lock");
    let lock_file = match std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(_) => return T::default(),
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = lock_file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            return T::default(); // lock busy — skip this write
        }
        let out = f();
        unsafe {
            libc::flock(fd, libc::LOCK_UN);
        }
        out
    }
    #[cfg(not(unix))]
    {
        f()
    }
}

/// Records a stats entry. Fire-and-forget: if the lock cannot be acquired
/// due to contention, the write is dropped silently to prevent blocking or corruption.
pub fn record(config: &Config, entry: &StatsEntry) {
    if !config.telemetry.local_stats {
        return;
    }
    with_stats_lock(|| {
        let dir = stats_dir();
        if paths::ensure_private_dir(&dir).is_err() {
            return;
        }
        let path = day_file(entry.ts_ms);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            if let Ok(line) = serde_json::to_string(entry) {
                let _ = writeln!(f, "{line}");
            }
        }
    });
}

pub fn record_result(config: &Config, result: &ToolResult, out_tokens: u32) {
    let raw_tokens = result.meta.raw_bytes.map(|b| (b / 4) as u32).unwrap_or(0);
    let fallback = result.meta.notes.iter().any(|n| {
        n.contains("parse_fallback")
            || n.contains("output_compacted")
            || n.contains("generic_compactor")
    });
    record(
        config,
        &StatsEntry {
            ts_ms: now_ms(),
            tool: result.tool.to_string(),
            ok: result.meta.ok,
            out_tokens,
            raw_tokens,
            sh: result.tool == "sh",
            fallback,
        },
    );
}

pub fn audit(config: &Config, tool: &str, ok: bool, code: Option<&str>) {
    if !config.telemetry.audit_log {
        return;
    }
    with_stats_lock(|| {
        let dir = paths::cache_dir();
        if paths::ensure_private_dir(&dir).is_err() {
            return;
        }
        let path = dir.join("audit.jsonl");
        let event = serde_json::json!({
            "ts_ms": now_ms(),
            "tool": tool,
            "ok": ok,
            "code": code,
        });
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(f, "{event}");
        }
    });
}

pub fn load_all() -> Vec<StatsEntry> {
    let mut out = Vec::new();
    let dir = stats_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return out;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            for line in text.lines() {
                if let Ok(entry) = serde_json::from_str::<StatsEntry>(line) {
                    out.push(entry);
                }
            }
        }
    }
    out.sort_by_key(|e| e.ts_ms);
    out
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct GainSummary {
    pub calls: u64,
    pub out_tokens: u64,
    pub raw_tokens: u64,
    pub saved_tokens: u64,
    pub by_tool: BTreeMap<String, u64>,
}

pub fn gain(since_ms: Option<i64>) -> GainSummary {
    let mut s = GainSummary::default();
    for e in load_all() {
        if let Some(since) = since_ms {
            if e.ts_ms < since {
                continue;
            }
        }
        s.calls += 1;
        s.out_tokens += e.out_tokens as u64;
        s.raw_tokens += e.raw_tokens as u64;
        if e.raw_tokens > e.out_tokens {
            s.saved_tokens += (e.raw_tokens - e.out_tokens) as u64;
        }
        *s.by_tool.entry(e.tool).or_insert(0) += 1;
    }
    s
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DiscoverSummary {
    pub calls: u64,
    pub sh_calls: u64,
    pub fallback_calls: u64,
    pub per_tool: BTreeMap<String, u64>,
}

pub fn discover(since_ms: Option<i64>) -> DiscoverSummary {
    let mut s = DiscoverSummary::default();
    for e in load_all() {
        if let Some(since) = since_ms {
            if e.ts_ms < since {
                continue;
            }
        }
        s.calls += 1;
        if e.sh {
            s.sh_calls += 1;
        }
        if e.fallback {
            s.fallback_calls += 1;
        }
        *s.per_tool.entry(e.tool).or_insert(0) += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_with_stats_lock_contention_drops_write() {
        let dir = stats_dir();
        let _ = paths::ensure_private_dir(&dir);
        let lock_path = dir.join(".stats.lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = lock_file.as_raw_fd();
            let rc = unsafe { libc::flock(fd, libc::LOCK_EX) };
            assert_eq!(rc, 0);

            // While the lock is held exclusively, with_stats_lock should fail non-blocking
            // and return Default without executing the closure.
            let mut called = false;
            let result: bool = with_stats_lock(|| {
                called = true;
                true
            });
            assert!(
                !called,
                "closure should not be called under lock contention"
            );
            assert!(!result, "should return Default on lock contention");

            unsafe {
                libc::flock(fd, libc::LOCK_UN);
            }
        }
    }
}
