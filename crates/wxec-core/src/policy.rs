use std::path::{Component, Path, PathBuf};

use crate::error::{Result, WErr};
use crate::paths;

/// Canonicalize a path (existing or not) and enforce confinement to `root`
/// unless it exactly matches an allowlist entry (spec §8.3).
pub fn resolve(
    root: &Path,
    input: &str,
    allowlist: &[PathBuf],
    must_exist: bool,
) -> Result<PathBuf> {
    if input.is_empty() {
        return Err(WErr::invalid_args(
            "empty path",
            "pass a workspace-relative path",
        ));
    }
    if input.contains('\0') {
        return Err(WErr::new("outside_workspace", "path contains NUL byte"));
    }
    let raw = Path::new(input);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let resolved = paths::canonicalize_lenient(&joined)
        .map_err(|e| WErr::new("not_found", format!("cannot resolve '{input}': {e}")))?;

    if must_exist && !resolved.exists() {
        return Err(WErr::with_hint(
            "not_found",
            format!("'{input}' does not exist"),
            "use `w tree` or `w search` to locate it",
        ));
    }

    let within_root = resolved == root || resolved.starts_with(root);
    let allowed = allowlist
        .iter()
        .any(|a| resolved == *a || resolved.starts_with(a));
    if !within_root && !allowed {
        return Err(WErr::with_hint(
            "outside_workspace",
            format!("'{input}' resolves outside the workspace root"),
            "use a path relative to the workspace root",
        ));
    }
    Ok(resolved)
}

/// Canonicalize the workspace root itself.
pub fn canonical_root(root: &Path) -> Result<PathBuf> {
    root.canonicalize().map_err(|e| {
        WErr::new(
            "not_found",
            format!("workspace root {}: {e}", root.display()),
        )
    })
}

/// Lexically normalize a path (does not touch the filesystem).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn check_tier(have: crate::types::Tier, need: crate::types::Tier, tool: &str) -> Result<()> {
    if have < need {
        return Err(WErr::with_hint(
            "policy_denied",
            format!("tool '{tool}' requires tier {need}, session has {have}"),
            format!("start with --tier {need} (or set session.default_tier)"),
        ));
    }
    Ok(())
}
