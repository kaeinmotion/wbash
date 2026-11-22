# Security

## Path confinement

- Every path argument is resolved relative to the workspace root, canonicalized,
  and rejected if it escapes (`outside_workspace`).
- Absolute paths are accepted only when they canonicalize inside the root or a
  configured allowlist entry.
- Symlinks resolving outside the root are rejected. Writes create parent
  directories only inside the root.
- Handles are stored under the cache directory, never inside the workspace, with
  restrictive permissions.

## Trust

- Project config (`.wxec/config.toml`) and project rules/parsers are loaded only after the root is trusted.
- `w config show` prints effective config; `w config trust` records the canonical root in the user config (locked for cross-process updates).
- Untrusted project configuration is ignored, with an advisory note.
- TOML parser rules are trusted-root only, validated, size-limited, and must be regular files (symlinks ignored).

## Tiers

| Tier | Allows |
|---|---|
| `read_only` | tree, read, search, expand, git reads |
| `write_workspace` | + edit |
| `git_write` | + git commit/push |
| `execute` | + run/check; `sh` additionally requires `exec.allow_shell` |

Policy is enforced in the dispatcher before the tool runs; a denied call emits
`!policy_denied` and never reaches the filesystem or process layer. Sessions
default to `write_workspace` for CLI, and `git_write` for MCP server sessions (configurable via `--tier`). `batch` derives each call's requirement
individually; a denied call fails only its own block.

## Execution

- `run` is argv-only: no shell interpolation.
- `sh` is disabled by default (`exec.allow_shell=false`).
- Child environment is allowlisted (`PATH`, `HOME`, locale, toolchain vars,
  etc.); arbitrary env keys are ignored or rejected.
- Timeouts are clamped to `exec.max_timeout_ms`; output is capped at
  `exec.max_output_bytes` and marked `output_truncated`. Process groups are
  killed on timeout.
- Working directories are jailed to the workspace.

## Redaction and storage

- Redaction runs before handles are written and before results are emitted.
  AWS keys, tokens, passwords, private keys, and common credential assignments
  are replaced and counted (`meta.redactions`).
- Handles are byte-limited (`handles.max_bytes`) and TTL-limited
  (`handles.ttl_secs`); `w handles gc` enforces both immediately.
- Logs are off/minimal by default; `WXEC_LOG` enables redacted local logging.
- No network calls and no telemetry in v0.1.
