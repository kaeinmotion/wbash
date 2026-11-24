use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, WErr};
use crate::hash;
use crate::paths;

macro_rules! default_mod {
    ($name:ident { $($field:ident : $ty:ty = $val:expr),* $(,)? }) => {
        pub mod $name {
            $(pub fn $field() -> $ty { $val })*
        }
    };
}

default_mod!(session_defaults {
    default_tier: String = "write_workspace".to_string(),
    ledger_ttl_secs: u64 = 7200,
    serialize_requests: bool = true,
});

default_mod!(handles_defaults {
    ttl_secs: u64 = 86_400,
    max_bytes: u64 = 524_288_000,
    expand_max_tokens: u32 = 8000,
    max_handles: usize = 500,
});

default_mod!(exec_defaults {
    timeout_ms: u64 = 120_000,
    max_timeout_ms: u64 = 600_000,
    max_output_bytes: u64 = 10_485_760,
    allow_shell: bool = false,
    env_allow: Vec<String> = super::default_env_allow(),
    kill_grace_ms: u64 = 2000,
});

default_mod!(read_defaults {
    head_lines: u32 = 30,
    max_full_lines: u32 = 300,
});

default_mod!(search_defaults {
    default_limit: u32 = 20,
    max_limit: u32 = 200,
    context_max: u32 = 20,
});

default_mod!(batch_defaults {
    max_calls: usize = 16,
});

default_mod!(output_defaults {
    passthrough_tokens: u32 = 160,
    min_gain_pct: u32 = 15,
    verbosity: String = "minimal".to_string(),
    edit_confirm: String = "compact".to_string(),
});

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    pub passthrough_tokens: u32,
    pub min_gain_pct: u32,
    pub verbosity: String,
    pub edit_confirm: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            passthrough_tokens: output_defaults::passthrough_tokens(),
            min_gain_pct: output_defaults::min_gain_pct(),
            verbosity: output_defaults::verbosity(),
            edit_confirm: output_defaults::edit_confirm(),
        }
    }
}

default_mod!(index_defaults {
    enabled: bool = true,
    languages: Vec<String> = super::default_languages(),
    max_tree_entries: usize = 250_000,
    use_trigrams: bool = false,
});

pub fn default_env_allow() -> Vec<String> {
    [
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TERM",
        "COLORTERM",
        "TMPDIR",
        "USER",
        "SHELL",
        "CARGO_*",
        "RUSTFLAGS",
        "RUSTUP_*",
        "RUSTC*",
        "CC",
        "CXX",
        "LD*",
        "GOPATH",
        "GOROOT",
        "GOCACHE",
        "GOMODCACHE",
        "GOFLAGS",
        "JAVA_HOME",
        "MAVEN_*",
        "GRADLE_*",
        "DOTNET_*",
        "NODE_*",
        "NPM_CONFIG_*",
        "NVM_DIR",
        "PNPM_HOME",
        "PYTHONPATH",
        "VIRTUAL_ENV",
        "PYENV_*",
        "POETRY_*",
        "UV_*",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_AUTHOR_NAME",
        "GIT_AUTHOR_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_COMMITTER_EMAIL",
        "SSH_AUTH_SOCK",
        "SSH_AGENT_PID",
        "GIT_SSH*",
        "GIT_CONFIG*",
        "XDG_*",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub fn default_languages() -> Vec<String> {
    ["rust", "typescript", "tsx", "python", "go"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub default_tier: String,
    pub ledger_ttl_secs: u64,
    pub serialize_requests: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            default_tier: session_defaults::default_tier(),
            ledger_ttl_secs: session_defaults::ledger_ttl_secs(),
            serialize_requests: session_defaults::serialize_requests(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HandlesConfig {
    pub ttl_secs: u64,
    pub max_bytes: u64,
    pub expand_max_tokens: u32,
    pub max_handles: usize,
}

impl Default for HandlesConfig {
    fn default() -> Self {
        Self {
            ttl_secs: handles_defaults::ttl_secs(),
            max_bytes: handles_defaults::max_bytes(),
            expand_max_tokens: handles_defaults::expand_max_tokens(),
            max_handles: handles_defaults::max_handles(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecConfig {
    pub timeout_ms: u64,
    pub max_timeout_ms: u64,
    pub max_output_bytes: u64,
    pub allow_shell: bool,
    pub env_allow: Vec<String>,
    pub kill_grace_ms: u64,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            timeout_ms: exec_defaults::timeout_ms(),
            max_timeout_ms: exec_defaults::max_timeout_ms(),
            max_output_bytes: exec_defaults::max_output_bytes(),
            allow_shell: exec_defaults::allow_shell(),
            env_allow: exec_defaults::env_allow(),
            kill_grace_ms: exec_defaults::kill_grace_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReadConfig {
    pub head_lines: u32,
    pub max_full_lines: u32,
}

impl Default for ReadConfig {
    fn default() -> Self {
        Self {
            head_lines: read_defaults::head_lines(),
            max_full_lines: read_defaults::max_full_lines(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub default_limit: u32,
    pub max_limit: u32,
    pub context_max: u32,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_limit: search_defaults::default_limit(),
            max_limit: search_defaults::max_limit(),
            context_max: search_defaults::context_max(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EditConfig {
    pub post_check: bool,
}

impl Default for EditConfig {
    fn default() -> Self {
        Self { post_check: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchConfig {
    pub max_calls: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_calls: batch_defaults::max_calls(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct IndexConfig {
    pub enabled: bool,
    pub languages: Vec<String>,
    pub max_tree_entries: usize,
    pub use_trigrams: bool,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            enabled: index_defaults::enabled(),
            languages: index_defaults::languages(),
            max_tree_entries: index_defaults::max_tree_entries(),
            use_trigrams: index_defaults::use_trigrams(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelemetryConfig {
    pub local_stats: bool,
    pub audit_log: bool,
    pub network: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            local_stats: true,
            audit_log: false,
            network: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
#[derive(Default)]
pub struct PolicyConfig {
    pub path_allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub session: SessionConfig,
    pub handles: HandlesConfig,
    pub exec: ExecConfig,
    pub read: ReadConfig,
    pub search: SearchConfig,
    pub edit: EditConfig,
    pub batch: BatchConfig,
    pub index: IndexConfig,
    pub output: OutputConfig,
    pub telemetry: TelemetryConfig,
    pub policy: PolicyConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            session: SessionConfig::default(),
            handles: HandlesConfig::default(),
            exec: ExecConfig::default(),
            read: ReadConfig::default(),
            search: SearchConfig::default(),
            edit: EditConfig::default(),
            batch: BatchConfig::default(),
            index: IndexConfig::default(),
            output: OutputConfig::default(),
            telemetry: TelemetryConfig::default(),
            policy: PolicyConfig::default(),
        }
    }
}

impl Config {
    pub fn hash(&self) -> String {
        let text = toml::to_string(self).unwrap_or_default();
        hash::hash_str(&text)
    }

    pub fn validate(&self) -> Result<()> {
        self.session.default_tier.parse::<crate::types::Tier>()?;
        if self.exec.timeout_ms == 0 || self.exec.timeout_ms > self.exec.max_timeout_ms {
            return Err(WErr::new(
                "invalid_config",
                format!(
                    "exec.timeout_ms={} must be >0 and <= exec.max_timeout_ms={}",
                    self.exec.timeout_ms, self.exec.max_timeout_ms
                ),
            ));
        }
        if self.search.default_limit == 0 || self.search.default_limit > self.search.max_limit {
            return Err(WErr::new(
                "invalid_config",
                "search.default_limit must be >0 and <= search.max_limit",
            ));
        }
        if self.read.max_full_lines == 0 {
            return Err(WErr::new(
                "invalid_config",
                "read.max_full_lines must be >0",
            ));
        }
        if !matches!(
            self.output.verbosity.as_str(),
            "minimal" | "normal" | "verbose"
        ) {
            return Err(WErr::new(
                "invalid_config",
                "output.verbosity must be minimal|normal|verbose",
            ));
        }
        if !matches!(self.output.edit_confirm.as_str(), "compact" | "detailed") {
            return Err(WErr::new(
                "invalid_config",
                "output.edit_confirm must be compact|detailed",
            ));
        }
        if self.telemetry.network {
            return Err(WErr::new(
                "invalid_config",
                "telemetry.network is not implemented in v0.1 and must remain false",
            ));
        }
        Ok(())
    }

    pub fn allowlist_paths(&self) -> Vec<PathBuf> {
        self.policy
            .path_allowlist
            .iter()
            .filter_map(|p| paths::canonicalize_lenient(Path::new(p)).ok())
            .collect()
    }
}

/// Load defaults < user < trusted project. Returns the effective config and a
/// list of human-readable warnings (e.g. untrusted project config).
pub fn load(
    root: &Path,
    explicit: Option<&Path>,
    no_project: bool,
) -> Result<(Config, Vec<String>)> {
    let mut warnings = Vec::new();
    let mut merged = toml_table(Config::default())?;

    let user_path = explicit
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("WXEC_CONFIG").map(PathBuf::from))
        .unwrap_or_else(|| paths::config_dir().join("config.toml"));
    if user_path.exists() {
        merge_file(&mut merged, &user_path)?;
    }

    if !no_project {
        let project = root.join(".wxec").join("config.toml");
        if project.exists() {
            if is_trusted(root) {
                merge_file(&mut merged, &project)?;
            } else {
                warnings.push(format!(
                    "project config {} ignored (untrusted); run `w config trust` or pass --no-project-config",
                    project.display()
                ));
            }
        }
    }

    let cfg: Config = toml::Value::Table(merged)
        .try_into()
        .map_err(|e| WErr::new("invalid_config", format!("config: {e}")))?;
    cfg.validate()?;
    Ok((cfg, warnings))
}

fn toml_table(cfg: Config) -> Result<toml::map::Map<String, toml::Value>> {
    let v = toml::Value::try_from(cfg)
        .map_err(|e| WErr::new("internal", format!("serialize config: {e}")))?;
    match v {
        toml::Value::Table(t) => Ok(t),
        _ => Err(WErr::internal("config did not serialize to a table")),
    }
}

fn merge_file(base: &mut toml::map::Map<String, toml::Value>, path: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        WErr::new(
            "invalid_config",
            format!("cannot read {}: {e}", path.display()),
        )
    })?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|e| WErr::new("invalid_config", format!("{}: {e}", path.display())))?;
    match value {
        toml::Value::Table(t) => {
            deep_merge(base, t);
            Ok(())
        }
        _ => Err(WErr::new(
            "invalid_config",
            format!("{}: expected a TOML table", path.display()),
        )),
    }
}

fn deep_merge(
    dst: &mut toml::map::Map<String, toml::Value>,
    src: toml::map::Map<String, toml::Value>,
) {
    for (k, v) in src {
        match (dst.get_mut(&k), v) {
            (Some(toml::Value::Table(dt)), toml::Value::Table(st)) => deep_merge(dt, st),
            (_, v) => {
                dst.insert(k, v);
            }
        }
    }
}

fn trust_file() -> PathBuf {
    paths::cache_dir().join("config").join("trust.json")
}

fn trust_set_unlocked() -> Vec<String> {
    std::fs::read_to_string(trust_file())
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

fn with_trust_lock<T>(f: impl FnOnce() -> T) -> T {
    let path = trust_file();
    if let Some(parent) = path.parent() {
        let _ = paths::ensure_private_dir(parent);
    }
    let lock_path = path.with_extension("lock");
    if let Ok(lock_file) = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = lock_file.set_permissions(std::fs::Permissions::from_mode(0o600));
        }
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let rc = unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_EX) };
            if rc == 0 {
                let out = f();
                unsafe { libc::flock(lock_file.as_raw_fd(), libc::LOCK_UN) };
                return out;
            }
        }
    }
    f()
}

pub fn is_trusted(root: &Path) -> bool {
    let root = paths::canonicalize_lenient(root).unwrap_or_else(|_| root.to_path_buf());
    let key = hash::hash_str(&root.to_string_lossy());
    with_trust_lock(|| trust_set_unlocked().contains(&key))
}

pub fn trust_root(root: &Path) -> Result<()> {
    let root = paths::canonicalize_lenient(root).map_err(|e| {
        WErr::new(
            "not_found",
            format!("cannot resolve {}: {e}", root.display()),
        )
    })?;
    let key = hash::hash_str(&root.to_string_lossy());
    with_trust_lock(|| {
        let mut set = trust_set_unlocked();
        if !set.contains(&key) {
            set.push(key);
        }
        let path = trust_file();
        if let Some(parent) = path.parent() {
            paths::ensure_private_dir(parent).map_err(WErr::from)?;
        }
        let bytes = serde_json::to_vec_pretty(&set)?;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = path.with_extension(format!("tmp-{}-{nonce}", std::process::id()));
        let res = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
            }
            f.write_all(&bytes)?;
            f.sync_all()?;
            Ok(())
        })();
        if let Err(e) = res {
            let _ = std::fs::remove_file(&tmp);
            return Err(WErr::from(e));
        }
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            WErr::from(e)
        })
    })
}

/// Render effective config with no secret values (there are none today).
pub fn render(cfg: &Config) -> String {
    toml::to_string_pretty(cfg).unwrap_or_else(|_| String::new())
}

pub fn counts_map(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}
