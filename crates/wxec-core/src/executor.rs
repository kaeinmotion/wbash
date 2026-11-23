use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::error::{Result, WErr};
use crate::redact::Redactor;
use crate::types::Exit;

#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub argv: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub timeout_ms: u64,
    pub stdin: Option<Vec<u8>>,
    pub env: HashMap<String, String>,
    pub max_output_bytes: u64,
    pub kill_grace_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub exit: Exit,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub duration_ms: u64,
    pub truncated: bool,
    pub timed_out: bool,
}

impl ExecOutput {
    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }

    pub fn combined(&self) -> String {
        let mut s = self.stdout_str();
        let err = self.stderr_str();
        if !err.is_empty() {
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            s.push_str("[stderr]\n");
            s.push_str(&err);
        }
        s
    }
}

fn env_allowed(allow: &[String], key: &str) -> bool {
    allow.iter().any(|a| {
        if let Some(prefix) = a.strip_suffix('*') {
            key.get(..prefix.len())
                .map(|s| s.eq_ignore_ascii_case(prefix))
                .unwrap_or(false)
        } else {
            a.eq_ignore_ascii_case(key)
        }
    })
}

/// Build the scrubbed child environment: inherited allowlisted keys plus
/// explicit per-call keys (which must be allowed and are never secret-named).
pub fn build_env(
    config: &Config,
    per_call: Option<&HashMap<String, String>>,
) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for (k, v) in std::env::vars_os() {
        let Ok(k) = k.into_string() else {
            continue;
        };
        let Ok(v) = v.into_string() else {
            continue;
        };
        if env_allowed(&config.exec.env_allow, &k) {
            env.insert(k, v);
        }
    }
    if !env.contains_key("PATH") {
        env.insert(
            "PATH".to_string(),
            "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin".to_string(),
        );
    }
    if let Some(path) = env.get_mut("PATH") {
        if let Ok(home) = std::env::var("HOME") {
            let cargo_bin = format!("{home}/.cargo/bin");
            if !path.split(':').any(|p| p == cargo_bin) && std::path::Path::new(&cargo_bin).is_dir()
            {
                *path = format!("{cargo_bin}:{path}");
            }
        }
    }
    if let Some(extra) = per_call {
        for (k, v) in extra {
            if !env_allowed(&config.exec.env_allow, k) {
                return Err(WErr::with_hint(
                    "policy_denied",
                    format!("env key '{k}' is not allowed"),
                    "add it to exec.env_allow in wxec config",
                ));
            }
            if Redactor::is_secret_key(k) {
                return Err(WErr::with_hint(
                    "policy_denied",
                    format!("env key '{k}' looks secret and is denied"),
                    "pass secrets through the parent environment, not tool args",
                ));
            }
            env.insert(k.clone(), v.clone());
        }
    }
    Ok(env)
}

/// Execute argv directly (no shell), with timeout, output caps and
/// process-group kill. Returns captured output even on timeout.
pub fn execute(req: &ExecRequest) -> Result<ExecOutput> {
    if req.argv.is_empty() || req.argv[0].is_empty() {
        return Err(WErr::invalid_args(
            "argv must contain at least one non-empty element",
            "pass argv=[program, args...]",
        ));
    }
    if !req.cwd.is_dir() {
        return Err(WErr::new(
            "cwd_not_found",
            format!("cwd {} is not a directory", req.cwd.display()),
        ));
    }

    let start = Instant::now();
    let mut cmd = Command::new(&req.argv[0]);
    let has_stdin = req.stdin.is_some();
    cmd.args(&req.argv[1..])
        .current_dir(&req.cwd)
        .env_clear()
        .envs(&req.env)
        .stdin(if has_stdin {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // New process group so we can kill the whole tree.
                if libc::setpgid(0, 0) != 0 {
                    // Non-fatal: fall back to killing the direct child.
                }
                Ok(())
            });
        }
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return match e.kind() {
                std::io::ErrorKind::NotFound => Err(WErr::with_hint(
                    "exec_not_found",
                    format!("executable '{}' not found", req.argv[0]),
                    "check argv[0] or PATH",
                )),
                std::io::ErrorKind::PermissionDenied => Err(WErr::new(
                    "permission_denied",
                    format!("cannot execute '{}': {e}", req.argv[0]),
                )),
                _ => Err(WErr::internal(format!("spawn '{}': {e}", req.argv[0]))),
            }
        }
    };

    let cap = req.max_output_bytes as usize;
    let stdout_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let stderr_buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let truncated = Arc::new(AtomicBool::new(false));

    let out = child.stdout.take();
    let err = child.stderr.take();
    let t_flag = truncated.clone();
    let out_thread = out.map(|pipe| spawn_reader(pipe, stdout_buf.clone(), cap, t_flag.clone()));
    let t_flag2 = truncated.clone();
    let err_thread = err.map(|pipe| spawn_reader(pipe, stderr_buf.clone(), cap, t_flag2));

    let stdin_thread = if has_stdin {
        child.stdin.take().map(|mut pipe| {
            let data = req.stdin.clone();
            std::thread::spawn(move || {
                if let Some(data) = data {
                    let _ = pipe.write_all(&data);
                }
            })
        })
    } else {
        None
    };

    let deadline = start + Duration::from_millis(req.timeout_ms.max(1));
    let mut timed_out = false;
    let mut spins: u32 = 0;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    timed_out = true;
                    #[cfg(unix)]
                    unsafe {
                        if let Ok(pid) = i32::try_from(child.id()) {
                            libc::kill(-pid, libc::SIGTERM);
                        }
                    }
                    let grace = Duration::from_millis(req.kill_grace_ms.max(1));
                    let grace_deadline = Instant::now() + grace;
                    while Instant::now() < grace_deadline {
                        if let Ok(Some(_)) = child.try_wait() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    #[cfg(unix)]
                    unsafe {
                        if let Ok(pid) = i32::try_from(child.id()) {
                            libc::kill(-pid, libc::SIGKILL);
                        }
                    }
                    let _ = child.kill();
                    match child.wait() {
                        Ok(s) => break Some(s),
                        Err(_) => break None,
                    }
                }
                if spins < 10 {
                    spins += 1;
                    std::thread::yield_now();
                } else if spins < 20 {
                    spins += 1;
                    std::thread::sleep(Duration::from_micros(250));
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
            Err(e) => return Err(WErr::internal(format!("wait: {e}"))),
        }
    };

    if let Some(t) = out_thread {
        let _ = t.join();
    }
    if let Some(t) = err_thread {
        let _ = t.join();
    }
    if let Some(t) = stdin_thread {
        let _ = t.join();
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = Arc::try_unwrap(stdout_buf)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_default();
    let stderr = Arc::try_unwrap(stderr_buf)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_default();

    let exit = if timed_out {
        Exit::Timeout
    } else {
        Exit::Code(status.and_then(|s| s.code()).unwrap_or(128))
    };

    Ok(ExecOutput {
        exit,
        stdout,
        stderr,
        duration_ms,
        truncated: truncated.load(Ordering::Relaxed),
        timed_out,
    })
}

fn spawn_reader(
    mut pipe: impl Read + Send + 'static,
    sink: Arc<Mutex<Vec<u8>>>,
    cap: usize,
    truncated: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let mut guard = sink.lock().unwrap_or_else(|e| e.into_inner());
                    let room = cap.saturating_sub(guard.len());
                    if room >= n {
                        guard.extend_from_slice(&chunk[..n]);
                    } else {
                        if room > 0 {
                            guard.extend_from_slice(&chunk[..room]);
                        }
                        truncated.store(true, Ordering::Relaxed);
                    }
                }
                Err(_) => break,
            }
        }
    })
}
