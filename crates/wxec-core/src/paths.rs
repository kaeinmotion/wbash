use std::path::{Path, PathBuf};

/// Resolve `$XDG_CONFIG_HOME` or `~/.config`.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(dir);
        if !p.as_os_str().is_empty() {
            return p.join("wxec");
        }
    }
    home().join(".config").join("wxec")
}

/// Resolve `$XDG_CACHE_HOME` or `~/.cache`.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        let p = PathBuf::from(dir);
        if !p.as_os_str().is_empty() {
            return p.join("wxec");
        }
    }
    home().join(".cache").join("wxec")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Canonicalize a path that may not exist yet: resolve the nearest existing
/// ancestor, then append the remaining (lexically normalized) components.
pub fn canonicalize_lenient(path: &Path) -> std::io::Result<PathBuf> {
    if let Ok(c) = path.canonicalize() {
        return Ok(c);
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut cur = abs.clone();
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(mut base) = cur.canonicalize() {
            for comp in suffix.iter().rev() {
                if comp == ".." {
                    base.pop();
                } else if comp != "." {
                    base.push(comp);
                }
            }
            return Ok(base);
        }
        match cur.file_name() {
            Some(name) => suffix.push(name.to_owned()),
            None => {
                // path ends in `..` or similar: consume the component name via iteration fallback
                if let Some(last) = cur.components().next_back() {
                    suffix.push(last.as_os_str().to_owned());
                } else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "cannot resolve path",
                    ));
                }
            }
        }
        match cur.parent() {
            Some(parent) => cur = parent.to_path_buf(),
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "cannot resolve path",
                ))
            }
        }
    }
}

pub fn relative_display(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        ".".to_string()
    } else {
        s
    }
}

/// Ensure a directory exists with 0700 permissions on unix.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}
