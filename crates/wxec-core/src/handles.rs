use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::error::{Result, WErr};
use crate::hash;
use crate::paths;
use crate::redact::Redactor;
use crate::types::{now_ms, HandleRecord, HandleRef};

pub struct HandleStore {
    dir: PathBuf,
    manifest: PathBuf,
    manifest_size: u64,
    handles: BTreeMap<String, HandleRecord>,
    next: u64,
    ttl_secs: u64,
    max_bytes: u64,
    session_hash: String,
    redactor: Redactor,
}

struct FlockGuard {
    fd: std::fs::File,
}

impl Drop for FlockGuard {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.fd.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

impl HandleStore {
    pub fn open(
        session_id: &str,
        root: &Path,
        ttl_secs: u64,
        max_bytes: u64,
        redactor: Redactor,
    ) -> Result<Self> {
        let session_hash = {
            let material = format!("{session_id}\u{0}{}", root.to_string_lossy());
            hash::hash_str(&material)[..16].to_string()
        };
        let base = paths::cache_dir().join("handles").join(&session_hash);
        paths::ensure_private_dir(&base)
            .map_err(|e| WErr::new("permission_denied", format!("handle dir: {e}")))?;
        let manifest = base.join("manifest.jsonl");
        let mut store = Self {
            dir: base,
            manifest,
            manifest_size: 0,
            handles: BTreeMap::new(),
            next: 1,
            ttl_secs,
            max_bytes,
            session_hash,
            redactor,
        };
        let _lock = store.lock(false)?;
        store.reload_manifest()?;
        drop(_lock);
        Ok(store)
    }

    pub fn put(&mut self, kind: &str, bytes: &[u8], content_type: &str) -> Result<HandleRef> {
        let _lock = self.lock(true)?;
        self.reload_manifest_if_changed()?;
        let (stored, redacted, ctype) = if content_type == "text" {
            let text = String::from_utf8_lossy(bytes);
            let (red, n) = self.redactor.redact(&text);
            (red.into_bytes(), n > 0, "text".to_string())
        } else {
            (bytes.to_vec(), false, "bytes".to_string())
        };
        let id = format!("~{}", base36(self.next));
        self.next += 1;
        let full = hash::hash_bytes(&stored);
        let now = now_ms();
        let rec = HandleRecord {
            id: id.clone(),
            kind: kind.to_string(),
            bytes: stored.len() as u64,
            hash: full.clone(),
            created_ms: now,
            expires_ms: now + (self.ttl_secs as i64) * 1000,
            last_access_ms: now,
            redacted,
            content_type: ctype,
        };
        let blob = self.blob_path(&id);
        write_cache_atomic(&blob, &stored)?;
        let out = HandleRef {
            id: id.clone(),
            kind: rec.kind.clone(),
            bytes: rec.bytes,
            hash: rec.hash.clone(),
        };
        self.append_record_locked(&rec)?;
        self.handles.insert(id.clone(), rec);
        self.evict_if_needed_locked()?;
        if self.total_bytes() > self.max_bytes || !self.handles.contains_key(&id) {
            let _ = std::fs::remove_file(self.blob_path(&id));
            self.remove_locked(&id);
            self.rewrite_manifest_locked()?;
            return Err(WErr::with_hint(
                "handle_too_large",
                format!("handle payload ({} bytes) exceeds cache limit", out.bytes),
                "increase handles.max_bytes or reduce output size",
            ));
        }
        Ok(out)
    }

    pub fn get(&mut self, id: &str) -> Result<(Vec<u8>, HandleRecord)> {
        let _lock = self.lock(false)?;
        self.reload_manifest_if_changed()?;
        let now = now_ms();
        let rec = self.handles.get(id).cloned().ok_or_else(|| {
            WErr::with_hint(
                "handle_not_found",
                format!("no handle '{id}' in this session"),
                "re-run the tool that produced it",
            )
        })?;
        if now > rec.expires_ms {
            drop(_lock);
            let _x = self.lock(true)?;
            self.reload_manifest_if_changed()?;
            let rec2 = self.handles.get(id).cloned();
            if rec2.as_ref().is_some_and(|r| now_ms() > r.expires_ms) || rec2.is_none() {
                self.remove_locked(id);
                self.rewrite_manifest_locked()?;
            }
            return Err(WErr::with_hint(
                "handle_expired",
                format!("handle '{id}' expired"),
                "re-run the tool that produced it",
            ));
        }
        let blob = self.blob_path(id);
        let bytes = match std::fs::read(&blob) {
            Ok(b) => b,
            Err(_) => {
                drop(_lock);
                let _x = self.lock(true)?;
                self.reload_manifest_if_changed()?;
                self.remove_locked(id);
                self.rewrite_manifest_locked()?;
                return Err(WErr::with_hint(
                    "handle_expired",
                    format!("handle '{id}' payload is gone"),
                    "re-run the tool that produced it",
                ));
            }
        };
        if let Some(h) = self.handles.get_mut(id) {
            h.last_access_ms = now;
        }
        let rec = self.handles.get(id).cloned().unwrap_or(HandleRecord {
            last_access_ms: now,
            ..rec
        });
        Ok((bytes, rec))
    }

    pub fn has_fresh(&mut self, id: &str) -> bool {
        let _lock = match self.lock(false) {
            Ok(l) => l,
            Err(_) => return false,
        };
        if self.reload_manifest_if_changed().is_err() {
            return false;
        }
        let Some(rec) = self.handles.get(id) else {
            return false;
        };
        let now = now_ms();
        if now > rec.expires_ms {
            drop(_lock);
            if let Ok(_x) = self.lock(true) {
                let _ = self.reload_manifest_if_changed();
                let stale = self.handles.get(id).is_none_or(|r| now_ms() > r.expires_ms);
                if stale {
                    self.remove_locked(id);
                    let _ = self.rewrite_manifest_locked();
                }
            }
            return false;
        }
        let blob = self.blob_path(id);
        if std::fs::metadata(&blob).is_ok() {
            return true;
        }
        drop(_lock);
        if let Ok(_x) = self.lock(true) {
            let _ = self.reload_manifest_if_changed();
            self.remove_locked(id);
            let _ = self.rewrite_manifest_locked();
        }
        false
    }

    pub fn session_hash(&self) -> &str {
        &self.session_hash
    }

    pub fn total_bytes(&self) -> u64 {
        self.handles.values().map(|h| h.bytes).sum()
    }

    pub fn gc(&mut self) -> u64 {
        let _lock = match self.lock(true) {
            Ok(l) => l,
            Err(_) => return 0,
        };
        let _ = self.reload_manifest();
        let removed = self.gc_locked();
        let _ = self.rewrite_manifest_locked();
        drop(_lock);
        removed
    }

    fn gc_locked(&mut self) -> u64 {
        let now = now_ms();
        let expired: Vec<String> = self
            .handles
            .values()
            .filter(|h| now > h.expires_ms)
            .map(|h| h.id.clone())
            .collect();
        let mut removed = expired.len() as u64;
        for id in expired {
            self.remove_locked(&id);
        }
        while self.total_bytes() > self.max_bytes {
            let victim = self
                .handles
                .values()
                .min_by_key(|h| h.last_access_ms)
                .map(|h| h.id.clone());
            match victim {
                Some(id) => {
                    self.remove_locked(&id);
                    removed += 1;
                }
                None => break,
            }
        }
        removed
    }

    fn evict_if_needed_locked(&mut self) -> Result<()> {
        if self.total_bytes() > self.max_bytes {
            self.gc_locked();
            self.rewrite_manifest_locked()?;
        }
        Ok(())
    }

    fn remove_locked(&mut self, id: &str) {
        self.handles.remove(id);
        let _ = std::fs::remove_file(self.blob_path(id));
    }

    fn blob_path(&self, id: &str) -> PathBuf {
        self.dir
            .join(format!("{}.blob", id.trim_start_matches('~')))
    }

    fn lock(&self, exclusive: bool) -> Result<FlockGuard> {
        let lock_path = self.dir.join(".lock");
        let f = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| {
                WErr::new(
                    "permission_denied",
                    format!("lock {}: {e}", lock_path.display()),
                )
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        let op = if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        };
        let rc = unsafe { libc::flock(f.as_raw_fd(), op) };
        if rc != 0 {
            return Err(WErr::new(
                "internal",
                format!(
                    "flock {}: {}",
                    lock_path.display(),
                    std::io::Error::last_os_error()
                ),
            ));
        }
        Ok(FlockGuard { fd: f })
    }

    fn reload_manifest_if_changed(&mut self) -> Result<()> {
        let size = std::fs::metadata(&self.manifest)
            .map(|m| m.len())
            .unwrap_or(0);
        if size != self.manifest_size || self.handles.is_empty() {
            self.reload_manifest()?;
        }
        Ok(())
    }

    fn reload_manifest(&mut self) -> Result<()> {
        let mut handles = BTreeMap::new();
        let mut next = self.next.max(1);
        let mut size = 0u64;
        if let Ok(meta) = std::fs::metadata(&self.manifest) {
            size = meta.len();
        }
        if let Ok(text) = std::fs::read_to_string(&self.manifest) {
            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(rec) = serde_json::from_str::<HandleRecord>(line) {
                    next = next.max(parse_id(&rec.id).map(|n| n + 1).unwrap_or(1));
                    handles.insert(rec.id.clone(), rec);
                }
            }
        }
        self.manifest_size = size;
        self.handles = handles;
        self.next = next;
        Ok(())
    }

    fn append_record_locked(&mut self, rec: &HandleRecord) -> Result<()> {
        let mut line = serde_json::to_string(rec).map_err(|e| WErr::internal(e.to_string()))?;
        line.push('\n');
        let bytes = line.as_bytes();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.manifest)
            .map_err(|e| {
                WErr::new(
                    "permission_denied",
                    format!("append {}: {e}", self.manifest.display()),
                )
            })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        if self.manifest_size > 0 {
            if let Ok(mut rf) = std::fs::File::open(&self.manifest) {
                use std::io::{Read, Seek, SeekFrom};
                if rf.seek(SeekFrom::End(-1)).is_ok() {
                    let mut b = [0u8; 1];
                    if rf.read_exact(&mut b).is_ok() && b[0] != b'\n' {
                        f.write_all(b"\n")?;
                        self.manifest_size += 1;
                    }
                }
            }
        }
        f.write_all(bytes)?;
        self.manifest_size += bytes.len() as u64;
        Ok(())
    }

    fn rewrite_manifest_locked(&mut self) -> Result<()> {
        let mut text = String::new();
        for rec in self.handles.values() {
            if let Ok(line) = serde_json::to_string(rec) {
                text.push_str(&line);
                text.push('\n');
            }
        }
        write_cache_atomic(&self.manifest, text.as_bytes())?;
        self.manifest_size = text.len() as u64;
        Ok(())
    }
}

fn write_cache_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| WErr::internal(format!("path {} has no parent", path.display())))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(
        ".{}.tmp-{}-{nonce}",
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "f".to_string()),
        std::process::id()
    ));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        f.write_all(bytes)?;
        Ok(())
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(WErr::new(
            "permission_denied",
            format!("write {}: {e}", path.display()),
        ));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(WErr::new(
            "permission_denied",
            format!("rename {}: {e}", path.display()),
        ));
    }
    Ok(())
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let mut out = Vec::new();
    while n > 0 {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "0".to_string())
}

fn parse_id(id: &str) -> Option<u64> {
    let s = id.trim_start_matches('~');
    u64::from_str_radix(s, 36).ok()
}
