# enterprise security & governance specification

technical reference for workspace confinement, cryptographic verification, secret redaction, and permission tiers in wxec.

---

### **table of contents**

- [chapter 1: security philosophy & threat model](#chapter-1-security-philosophy--threat-model)
- [chapter 2: path confinement & workspace jailing](#chapter-2-path-confinement--workspace-jailing)
- [chapter 3: permission tiers & access control](#chapter-3-permission-tiers--access-control)
- [chapter 4: automated secret & credential redaction](#chapter-4-automated-secret--credential-redaction)
- [chapter 5: process isolation & execution boundaries](#chapter-5-process-isolation--execution-boundaries)
- [chapter 6: memory handle lifecycle & storage isolation](#chapter-6-memory-handle-lifecycle--storage-isolation)

---

### **chapter 1: security philosophy & threat model**

autonomous ai agents operating in software repositories present unique enterprise security challenges:
- **prompt injection risks**: malicious instructions embedded in issues, dependencies, or comments attempting to access external networks or exfiltrate private credentials.
- **uncontrolled filesystem escape**: relative path traversals (`../../`) accessing sensitive operating system configuration files (`/etc/passwd`, `~/.ssh`).
- **token log pollution**: raw terminal output containing api keys, database connection strings, or private certificates that leak into model inference history.

wxec implements defense-in-depth at the protocol layer, enforcing immutable safety policies before any tool request touches the underlying host.

---

### **chapter 2: path confinement & workspace jailing**

every file system operation is subjected to strict path verification:

1. **canonical root resolution**: all input paths are resolved relative to the workspace root and canonicalized, resolving all symbolic links.
2. **traversal rejection**: any path resolving outside the workspace root triggers an immediate `outside_workspace` error.
3. **symlink isolation**: symlinks pointing outside the workspace boundary are rejected for read, write, and search operations.
4. **directory creation boundaries**: parent directory creation during `edit` operations is confined strictly within the project boundary.

---

### **chapter 3: permission tiers & access control**

wxec enforces four hierarchical permission tiers configured at server launch via `--tier`:

| tier | level | authorized tools | operational boundary |
| :--- | :---: | :--- | :--- |
| `read_only` | 0 | `tree`, `read`, `search`, `expand`, `git` (`status`/`diff`/`log`) | non-mutating inspection; disk writes strictly forbidden |
| `write_workspace` | 1 | tier 0 + `edit`, `replace` | allows in-workspace file edits; process execution forbidden |
| `execute` | 2 | tier 1 + `run`, `check`, `sh` *(if enabled)* | allows running compilers, linters, tests, and build tools |
| `git_write` | 3 | tier 2 + `git` (`commit`/`push`) | full autonomous agent workflow including version control |

- **default tiers**: mcp server defaults to `git_write` (or configured via `.wxec.toml`).
- **batch evaluation**: in `batch` workflows, permissions are verified per child call. a denied call emits `!policy_denied` for its block without interrupting authorized siblings.

---

### **chapter 4: automated secret & credential redaction**

all tool outputs and handle store writes are scanned in real time before transmission over mcp:

- **regexset fast-path**: high-speed vectorized pattern engine scrubs sensitive credentials:
  - modern ai secret keys (anthropic, openai, google gemini, huggingface).
  - cloud provider credentials (aws access/secret keys, gcp service accounts, azure tokens).
  - private keys (rsa, ed25519, openbsd openssh keys).
  - authorization headers (`bearer ...`, `basic ...`).
  - generic passwords, connection strings, and oauth tokens.
- **redaction accounting**: redacted fields are replaced with `[REDACTED]` markers, and counts are tracked in execution metadata (`meta.redactions`).

---

### **chapter 5: process isolation & execution boundaries**

for tools operating under the `execute` tier (`run`, `check`, `sh`):

- **no shell interpolation in `run`**: arguments are executed directly via `execvp` syscalls as typed argv vectors, eliminating shell injection vectors.
- **environment allowlist**: child processes do not inherit the parent process environment. only safe, necessary variables are passed:
  - system paths: `PATH`, `HOME`, `USER`.
  - toolchain variables: `RUST_BACKTRACE`, `NODE_ENV`, `PYTHONUNBUFFERED`, `CARGO_HOME`.
  - sensitive credentials (e.g. `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) are stripped from child processes by default.
- **process termination**: hung or runaway child processes are killed after `timeout_ms` (default: 120 seconds). process groups are terminated to eliminate orphaned background jobs.

---

### **chapter 6: memory handle lifecycle & storage isolation**

- **isolated storage**: memory handles (`~id`) are stored in an operating system cache directory outside the workspace root, preventing accidental commits to git.
- **restricted permissions**: handle storage directories are initialized with user-only permissions (`0700` on unix).
- **automatic expiration**: handles are bounded by time-to-live (`handles.ttl_secs = 86400`) and maximum byte capacity (`handles.max_bytes = 104857600`). automated garbage collection purges stale entries.
