# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

## Security Philosophy

`wxec` is designed as a secure, sandboxed, and token-efficient execution layer for AI coding agents. Security is enforced through defense-in-depth:

1. **Path Confinement**: All file paths are jailed strictly within the workspace root. Path traversal escapes (`..`), symlinks resolving outside the root, and NUL-byte path injection are unconditionally blocked with `outside_workspace`.
2. **Deterministic Tier Policy**: Tool capabilities are partitioned into distinct security tiers (`read_only`, `write_workspace`, `execute`, `git_write`). Denied calls fail immediately with `!policy_denied` before touching the filesystem or process table.
3. **Automatic Credential Redaction**: Secret scanners automatically intercept AWS keys, bearer tokens, passwords, private keys, and environment secrets before emitting results or storing handles.
4. **Child Process Sandboxing**:
   - `run` executes `argv` directly without shell interpolation.
   - Arbitrary shell execution (`sh`) is disabled by default.
   - Environment variables are allowlisted, stripping sensitive host keys.
   - Execution timeouts and maximum output limits are enforced with process-group kill guarantees.

## Reporting a Vulnerability

If you discover a security vulnerability in `wxec`, please report it privately:

- **GitHub Security Advisories**: Report via the private advisory tab at [https://github.com/kaeinmotion/wxec/security/advisories/new](https://github.com/kaeinmotion/wxec/security/advisories/new).
- Do not disclose security issues in public GitHub issues or discussions until they have been reviewed and addressed.
- Reports will be acknowledged within 48 hours with an initial assessment and resolution timeline.
