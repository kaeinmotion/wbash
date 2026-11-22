# w bash — Implementation-Ready Specification (v3.2)

**Status:** Implementation (v0.2.1 baseline, token-efficiency enhancements)  
**Version:** spec 3.2 / wire `lean@0.2` (extended) / config `schema_version=1`  
**Date:** 2026-11-17  
**Supersedes:** v3.1

This document is the normative source for v0.1 implementation, testing, packaging, and release gating.

---

## 0. Product definition

**w bash** is a local tool server and CLI for AI agents (in developer tools or inside systems). It performs workspace, execution, and VCS operations and returns compact, deterministic results so agents use fewer tokens while keeping important details and access to full output by reference.

| Surface | Identifier | Notes |
|---|---|---|
| Product name | `w bash` | prose/branding |
| CLI binary | `w` | primary local interface |
| MCP server name | `wxec` | stdio transport (v0.1) |
| Crates | `wxec-core`, `wxec-tools`, `wxec-index`, `wxec-parsers`, `wxec-adapters`, `wxec-cli` | single workspace, one static binary |
| User config | `$XDG_CONFIG_HOME/wxec/config.toml` else `~/.config/wxec/config.toml` | |
| Project config | `<root>/.wxec/config.toml` | inactive until trusted |
| Cache/data | `$XDG_CACHE_HOME/wxec/` else `~/.cache/wxec/` | handles, indexes, stats, audit |
| Env prefix | `WXEC_*` | runtime overrides/logging |
| Handle ref | `~<id>` where `<id>` is `[a-z0-9]{1,8}` | session-scoped, immutable |
| Content hash display | `b3:<16hex>` | BLAKE3, full hash stored internally |

**Supported platforms (v0.1):** Linux x86_64/aarch64, macOS x86_64/aarch64. Windows: WSL supported; native Windows best-effort/deferred (no v0.1 gate).  
**License:** MIT.

---

## 1. Normative language, goals, non-goals, gates

### 1.1 Normative keywords
`MUST`, `MUST NOT`, `SHOULD`, `SHOULD NOT`, `MAY` are interpreted per RFC 2119/8174 when capitalized.

### 1.2 Goals
1. **Efficiency:** ≥50% mean output-token reduction per task vs plain-bash baseline (scripted benchmark; report real `cl100k_base` as primary and chars/4 as secondary). Token efficiency is a correctness property: noisy-of-signal and lossy-of-signal outputs are failures.
2. **Correctness:** task success ≥ baseline − 2 percentage points; turns ≤ baseline.
3. **Safety of meaning:** 0 dropped-error incidents (errors/failures/exit/timeout/terminal failure signals and available error/fail facts such as locations, frames, and failing-test details preserved or explicitly counted in must-keep output). Semantic `ok` requires no errors/failures; warnings must still be surfaced as counts.
4. **Determinism:** identical `(tool,args,config_hash,observed_state,session_state)` ⇒ byte-identical output (modulo explicitly allowed measured durations in live exec).
5. **Local-first:** no in-loop LLM, no default network telemetry.
6. **Extensibility:** common tool normalization via declarative TOML parsers/rules without core changes.

### 1.3 Non-goals (v0.1)
- Replacing host agent sandbox/permission UX or bypassing destructive-action consent.
- LLM summarization.
- Universal parser/ecosystem coverage.
- Remote/multi-tenant service, cloud sync.
- WASM plugins, local HTTP MCP transport, unified-patch edit input, native Windows parity, network telemetry.

### 1.4 Release gates (blocking)
| ID | Gate | Target | Evidence |
|---|---|---|---|
| G1 | Task token reduction | ≥50% mean vs bash | `bench/` paired runs, ≥30 tasks × ≥5 runs |
| G2 | Task success | ≥ baseline − 2 pp | same harness |
| G3 | Turns | ≤ baseline | same harness |
| G4 | Dropped-error incidents | 0 | invariant/property/differential suites |
| G5 | MCP schema budget | ≤1500 tokens total | CI count of exposed tool names+descriptions+schemas |
| G6 | `read` latency p95 warm | <5 ms | perf bench |
| G7 | `search` latency p95 warm, 50k files | <50 ms | perf bench, warm index or documented fallback |
| G8 | Cold start `w` / `wxec` | <50 ms / <200 ms | perf bench |
| G9 | Token estimator MAPE | ≤10% | corpus vs pinned tokenizer |
| G10 | Security | 0 known path-jail/policy escapes | fuzz + policy matrix |

---

## 2. System architecture

### 2.1 Components and boundaries
| Layer | Crate | Responsibility | MUST NOT |
|---|---|---|---|
| L1 Adapters | `wxec-adapters`, `wxec-cli` | MCP stdio framing, CLI arg parsing/exit codes, hook mapping, session propagation | contain tool/domain logic |
| L2 Dispatcher | `wxec-core` | validate→resolve→policy→execute→budget-fit→format→record | perform transport I/O specifics |
| L3 Tools | `wxec-tools` | tool semantics; emit `ToolResult` | emit wire text/JSON |
| L4 Services | `wxec-core/index/parsers` | workspace/index, ledger, handles, executor, parsers, formatter, estimator, policy, stats, audit | depend on adapters |
| L5 Platform | (within services) | FS, process, git (`gix`), watcher | leak OS details into tool contracts |

### 2.2 Request pipeline (normative order)
1. **Ingress:** adapter receives `{session_id?, tool, args, call_id?}`.
2. **Schema validation:** tool exists, args match schema (types, required, enums, ranges), **unknown fields rejected** → `!invalid_args`.
3. **Session resolve:** `(session_id || "default", canonical_root)`; create/load session; update `last_seen`.
4. **Policy gate:** capability tier, tool gates (`allow_shell`, git-write), path jail on every path-like arg (`path`, `cwd`, `pathspec`, git paths) → `!policy_denied` / `!outside_workspace`.
5. **Execute:** tool returns `ToolResult`.
6. **Budget fit:** apply §7.1; preserve must-keep; produce elisions/handles.
7. **Format:** `lean` or `json` via formatter only.
8. **Record:** persist handles/manifests, update ledger and stats, append audit event if enabled.
9. **Egress:** adapter returns response; CLI maps to stdout/stderr/exit code per §4.5.

### 2.3 Concurrency and consistency
- Requests within a session are **serialized** by default (`session.serialize_requests=true`) to preserve ledger determinism.
- `batch` MAY execute child calls concurrently when independent; results MUST be emitted in input order.
- Indexing is asynchronous/incremental; if unavailable, tools MUST use exact fallback and MAY emit `# index_warming fallback_scan` (advisory, not an error, not silent incompleteness).
- Writes/edits are atomic; handle and audit persistence are append/rename-safe.

---

## 3. Data model and storage

### 3.1 Types (conceptual, normative fields)
```text
Session {
  id: String, root: AbsPath, tier: Tier, config_hash: B3,
  started_at_ms: i64, last_seen_ms: i64, ledger: Ledger
}
Tier = read_only < write_workspace < execute < git_write  (ordered)

Ledger { reads: Map<PathScopeKey, ReadSeen>, searches: Map<HashKey, SearchSeen>, runs: Map<HashKey, RunSeen> }
ReadSeen   { full_hash: [u8;32], short: "b3:<16hex>", mtime_ns: i64, size: u64, scope: Scope, shown_at_ms: i64 }
SearchSeen { query_hash: [u8;32], result_hash: [u8;32], shown_at_ms: i64 }
RunSeen    { command_hash: [u8;32], exit: Exit, results: Map<String,TestStatus>, shown_at_ms: i64 }
Scope      = Full | Lines{start:u32,end:u32} | Symbol{name:String} | Outline

Handle {
  id: String, session_id: String, kind: tree|read|search|run|check|git|sh|batch|expand,
  bytes: u64, full_hash: [u8;32], created_at_ms: i64, expires_at_ms: i64,
  last_access_ms: i64, redacted: bool, content_type: "text"|"bytes"
}

Diagnostic { file: String, line: u32, col?: u32, end_line?: u32, end_col?: u32,
             severity: error|warn|info, code?: String, message: String, tool?: String }
TestResult { id: String, status: pass|fail|skip|xfail|error|timeout,
             duration_ms?: u64, message?: String, location?: String }

Item { class: must_keep|high|low, value: f32, est_tokens: u32, render: RenderNode }
ToolMeta { tool: String, ok: bool, exit?: Exit, duration_ms?: u64,
           counts?: Map<String,i64>, redactions?: u32, notes: Vec<String>, truncated?: bool }
ToolResult { meta: ToolMeta, items: Vec<Item>, handles: Vec<HandleRef>, diagnostics: Vec<Diagnostic> }
Exit = { code: i32 } | { timeout: true }
```

### 3.2 Identity and canonicalization
- **Content hash:** BLAKE3 of exact bytes; display short form only in wire output.
- **PathScopeKey:** canonical workspace-relative path + normalized scope descriptor.
- **Query/command hash keys:** BLAKE3 of canonical JSON serialization (sorted keys, normalized paths, normalized argv/whitespace where semantic) of relevant args.
- **Handle IDs:** allocated monotonically per session from base36 counter; MUST NOT be reused within session; uniqueness key `(session_id,id)`.

### 3.3 On-disk layout and permissions
```text
$XDG_CACHE_HOME/wxec/
  handles/<session_hash>/<id>.blob      # raw (redacted) payload
  handles/<session_hash>/manifest.jsonl # Handle records (append-only)
  index/<root_hash>/meta.json
  index/<root_hash>/symbols.<lang>.bin
  index/<root_hash>/trigrams.bin        # optional
  stats/YYYY-MM-DD.jsonl                # local metrics only
  audit.jsonl                           # only if telemetry.audit_log=true
  sessions/<session_hash>.json          # optional snapshot for resume (v0.1 MAY omit)
```
- Directories `0700`, files `0600`. Project-local state MUST NOT be written except user-approved `.wxec/` config artifacts (none by default).
- **Handle eviction:** expire by TTL (default 24h) and LRU when total > `handles.max_bytes` (default 500 MiB). `expand` on missing/expired → `!handle_expired`.

### 3.4 Determinism inputs
Deterministic output depends on: normalized args, config hash, workspace bytes/metadata observed, ledger state, and index state (or documented fallback). Live `duration_ms` is a measured value and MUST be formatted deterministically (integer ms or fixed 1-decimal seconds as specified per tool; see §6).

---

## 4. Interfaces

### 4.1 CLI (`w`)
**General form:** `w [global-opts] <tool> [tool-opts] [positionals]`
- Global opts: `--root <path>`, `--session <id>` (default `default`), `--format lean|json` (default `lean`), `--budget <n>`, `--config <path>`, `--no-project-config`, `-q` (reserved, v0.1 no-op).
- Tool opts use `--kebab-case` mapped to schema names (`--path`, `--pattern`, `--timeout-ms`). Booleans `--outline` / `--no-outline`.
- Positional sugar (MUST be supported for ergonomics; equivalent to flags):
  - `w tree [path]`
  - `w read <path>`
  - `w search <pattern> [path]`
  - `w expand <ref>`
  - `w run -- <argv...>` or `w run <cmd> [args...]` (interpreted as argv vector, no shell)
  - `w git <op> [op-args...]`
- `w batch` accepts `--calls <json>` or `--calls-file <path>` (JSON array of `{tool,args}`).

**Exit codes (CLI):**
| Code | Meaning |
|---:|---|
| 0 | call succeeded (`meta.ok=true`) |
| 1 | tool/domain failure or `meta.ok=false` (e.g. `check` with errors) — response still printed |
| N | `run`/`sh` child exit code when nonzero (e.g. `101` for cargo); timeout maps to `124` |
| 2 | invocation/validation/policy/config error (no tool execution) |
| 70 | internal/unexpected error (`!internal`) |

**Streams:** primary result to stdout; CLI diagnostics (e.g. config parse failure) to stderr. Lean output MUST end with `\n`.

### 4.2 MCP (`wxec`, stdio v0.1)
- Transport: MCP stdio JSON-RPC. Server name `wxec`, version = binary version.
- Exposes one MCP tool per `w bash` tool with identical names and parameter schemas.
- Tool annotations: `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint=false` set accurately per §5 matrix.
- `session_id` propagation: adapter MUST accept session from MCP metadata/argument convention supported by host; if absent use `default`. (Implementation MUST document exact field; RECOMMENDED: optional `_meta.session_id` or `session` param if host requires, but prefer adapter-level.)
- Responses: `content` includes formatted lean text (or JSON text when `format=json`); `isError` true for invocation/policy/internal failures (CLI exit 2/70 equivalents). Tool-level failures with valid results (e.g. `exit 1`) SHOULD be `isError=false` with payload indicating failure. MCP does not propagate child exit codes; only the CLI does for `run`/`sh`.

### 4.3 Hook adapter (v0.1 optional, recommended)
- Maps host read/search/exec/test built-ins to `read|search|run|check` where safe.
- MUST preserve host permission semantics and MUST NOT widen policy tier.
- Emits `# via wxec` note in debug trace only (not in lean output).

### 4.4 Configuration precedence and trust
1. built-in defaults
2. user config (`~/.config/wxec/config.toml` or `$WXEC_CONFIG`)
3. project config (`.wxec/config.toml`) **only if trusted**
4. explicit CLI flags / invocation overrides

**Project trust:** on first encounter of untrusted project config, CLI MUST refuse to apply it and emit actionable instruction (e.g. `w config trust --root <root>` or equivalent) unless `--no-project-config`. Trust decision stored in user cache (`config/trust.json`, 0600). v0.1 MAY implement `--no-project-config` only and require manual trust file creation documented; either way behavior MUST be explicit and fail-closed.

### 4.5 Utility commands
| Command | Purpose |
|---|---|
| `w gain [--since <date|dur>] [--json]` | local token-savings summary |
| `w discover [--since <dur>]` | report `sh`/fallback usage and native-tool opportunities |
| `w parser test <parser.toml> <sample>` | validate parser mapping (stdout normalized result) |
| `w handles gc` | expire/evict handles now |
| `w config show` | effective config (secrets redacted) |

---

## 5. Tool specifications (normative)

### 5.1 Capability and hints matrix
| Tool | Min tier | readOnly | destructive | idempotent | Gate |
|---|---|---:|---:|---:|---|
| `tree` | read_only | yes | no | yes | — |
| `read` | read_only | yes | no | yes | — |
| `search` | read_only | yes | no | yes | — |
| `expand` | read_only | yes | no | yes | — |
| `git` (status/diff/log/show/blame/branch) | read_only | yes | no | yes | — |
| `edit` | write_workspace | no | conditional | conditional | — |
| `replace` | write_workspace | no | yes | no | — |
| `run` | execute | no | unknown | no | — |
| `check` | execute | no | no | mostly | — |
| `git` (commit/push) | git_write | no | yes | no | write ops |
| `batch` | max(children) | derived | derived | derived | per-child |
| `sh` | execute | no | unknown | no | `allow_shell=true` |

### 5.2 Common argument rules
- `budget` (uint, default per-tool: `tree/read/search/expand/git=600`, `run/check/sh=800`, `batch=sum(child defaults) capped`), min 64, max 32768.
- `format` = `lean|json`, default `lean`.
- `path`/`cwd` resolved against root; MUST pass path jail; output paths workspace-relative with `/` separators.
- `lines` range syntax: `<start>:<end>` 1-based inclusive; either side MAY be omitted (`:50`, `10:`) meaning file start/end; clamped with `# clamped <req> -> <actual>` note.
- Unknown args → `!invalid_args <tool>: unknown arg '<k>'. <hint>`.

### 5.3 `tree`
- **Args:** `path="."`, `depth=2` (int, 0=unlimited up to `index.max_tree_entries` guard), `glob?` (filter), `sizes=false`.
- **Behavior:** apply ignore rules (gitignore, global, built-ins `.git,node_modules,target,vendor,.next,dist` configurable); sort dirs-first then files, lexicographic UTF-8 byte-stable. Directories over inline threshold or beyond depth render collapsed as `name/ (N) ~h` with contents in a handle; small leaf directories at a depth boundary (no subdirectories, ≤5 files) may be shown inline. File groups beyond inline threshold render as `(N files) ~h`.
- **Header:** `<display_path>/ (N files[, M dirs])` or `<display_path>/`.
- **Errors:** `not_found`, `not_a_directory`, `outside_workspace`, `invalid_args`.

### 5.4 `read`
- **Args:** `path` (req), `lines?`, `symbol?`, `outline=false`, `expect_hash?`, `force=false`, `budget?`.
- **Precedence:** `symbol` > `lines` > `outline` > default (ledger-aware full/summary).
- **Default/ledger:** verify current bytes/hash; if matches `ReadSeen` for same scope → `unchanged <path> (b3:...) expand ~h` (handle omitted only if unavailable). `force=true` bypasses the ledger short-circuit and diff view, re-emitting the selected content. If changed and prior shown exists: emit compact diff (new-file line numbers, `@@` hunk headers allowed minimally) when `est(diff) < est(full)` and within budget, else selected full view. Always update ledger on success.
- **Rendering:** text as `<line>: <content>` (line 1-based). Binary: `binary <mime-or-kind> <bytes> bytes [sha256-prefix]` no body. Oversize/no scope: head `read.head_lines`, `...+N lines (~h)`, then outline symbols not already shown in the head.
- **Outline:** supported langs (Rust, TS/TSX, Python, Go) via tree-sitter signatures/types/doc-first-lines with indentation nesting; fallback heuristic labeled `# heuristic_outline`.
- **Symbol:** extract definition/body range for named symbol; if not found `!symbol_not_found symbol '<n>' in <path>. try: w search pattern=<n> mode=symbol` (or outline).
- **Errors:** `not_found`, `outside_workspace`, `binary_unsupported` (symbol/lines on binary unless bytes-safe range defined; v0.1: error), `symbol_not_found`, `hash_mismatch`, `invalid_args`.

### 5.5 `search`
- **Args:** `pattern` (req), `mode=text|regex|symbol` (default `text`), `path="."`, `glob?`, `context=0` (0–20), `limit=50` (≤`search.max_limit`), `budget?`.
- **Engine:** ripgrep/`grep-*` crates; exact verification; ignore-aware. `symbol` uses index when available else text/def-heuristic fallback with `# symbol_index_unavailable fallback_scan`.
- **Ranking (desc):** `is_definition` > `exact_case` > `ledger_proximity` > `recency_bucket` > `source_weight` (tests/vendor down-weighted unless in-scope/requested) > `match_quality`. Tie-break: `path,line,col`.
- **Output:** header `<F> file(s), <M> match(es)` with singular forms when counts are 1 (and `(<R> returned)` if capped). When no matches are found but binary files were skipped, add `# skipped <N> binary file(s)` so zero is not silently misleading. Results keep first-seen file order by rank; hits are sorted by line within file. A file with >=3 returned hits renders as `path (N):` with `  line: snippet` (or `  l1,l2: snippet` for identical adjacent snippets without context); files with <3 returned hits render flat `path:line: snippet`. Context lines prefixed `path:line- snippet`. Overflow `...+M more (~h)`.
- **Alternation fallback:** in text mode, if a literal scan finds zero matches and the pattern is a safe literal alternation (`a|b`, escaped branches), search retries as alternation and notes `# '|' alternation`; if no literal/alternation match is found and the pattern contains `|`, note `# '|' literal; try mode=regex`.
- **Binary detection:** NUL bytes mark a file binary. An incomplete trailing multi-byte sequence is text. Invalid UTF-8 without NUL decodes as Latin-1 text (edits preserve/re-encode encoding) so search/read/edit remain usable on legacy source.
- **Errors:** `invalid_regex` (with engine message/position), `outside_workspace`, `not_found` (path), `invalid_args`. Index not ready → results + `# index_warming fallback_scan`.

### 5.6 `edit`
- **Args:** `path` (req), exactly one mode + `expect_hash?`:
  - `find` (string, literal, non-empty) + `replace` (string)
  - `lines="s:e"` + `replace` (string)
  - `create=true` + `content` (string) [+ `overwrite=false`]
  - `delete=true`
- **Matching/atomicity:** `find` is **byte-exact literal** against current file bytes (no regex, no fuzzy whitespace) and MUST match exactly once else `!ambiguous_anchor <n> matches in <path> at <locs...>. try: w read path=<p> lines=<s>:<e>` (up to 5 locs). `lines` replace clamps invalid bounds → `!invalid_range` or clamp note per case (v0.1: invalid range errors). Apply via temp file in same dir + fsync + atomic rename.
- **Hash guard:** if `expect_hash` set and differs from current → `!hash_mismatch current=b3:...`.
- **Post-check:** if `edit.post_check` enabled, run fast diagnostics for touched path/lang when available; append `; <E> errors, <W> warnings new` (must-keep if errors) or store detail in handle. Check failure/timeout → `# post_check_unavailable <reason>` (edit still reported applied).
- **Output:** `created <path> (<bytes> bytes)` | `edited <path> (+ins -del)` (default compact; `output.edit_confirm=detailed` or verbose may include `(b3:new)`) | `deleted <path>` plus optional diag suffix.
- **Errors:** `not_found`, `already_exists`, `ambiguous_anchor`, `hash_mismatch`, `invalid_range`, `outside_workspace`, `permission_denied`, `invalid_args`, `is_directory`.

### 5.7 `replace`
- **Args:** `find` (req), `replace` (req), `path="."`, `paths[]`, `glob?`, `word=false`, `regex=false`, `dry_run=false`, `max_files=200`, `limit?`.
- **Matching:** literal byte/string matching by default; `word` wraps escaped literal with `\b`; `regex` uses the provided pattern. Replaces all matches (bounded by `limit` when set).
- **Scope/limits:** resolves paths within workspace, honors gitignore/walk and glob; skips binary files; rejects scope > `max_files` with `!too_many_files`.
- **Execution:** per-file atomic write (fsync+rename) unless `dry_run`; ledger invalidated and workspace cache updated per changed file.
- **Output:** `replaced <N> match(es) in <M> file(s)` or `dry_run: ...` plus per-file match counts (`path (N)`); with verbose output emits `path:line: -old`/`+new` diff lines (budget-fitted). 0 matches -> `!replace_not_found`.
- **Errors:** `invalid_args`, `invalid_regex`, `too_many_files`, `replace_not_found`, `outside_workspace`, `permission_denied`.

### 5.8 `run`
- **Args:** `argv` (string[], req, len≥1), `cwd?="."`, `timeout_ms?=120000` (≤`exec.max_timeout_ms`), `stdin?`, `env?` (map, keys must pass allowlist policy), `budget?`.
- **Execution:** direct exec (no shell). Resolve `argv[0]` via PATH in exec env. `cwd` jailed. Child env is allowlisted (including toolchain-relevant vars/patterns such as `CARGO_*`/`CARGO_TARGET_DIR`, Go/Python/Node/Java/Maven/Gradle families as configured) with secret-key denial enforced.
- **Capture/limits:** stream stdout/stderr to disk up to `exec.max_output_bytes`; keep bounded tail in memory; preserve order (PTY only if required, v0.1 default pipes). On timeout send TERM then KILL to process group; return partial handle.
- **Parsing/raw separation:** parsers MAY inject semantics-preserving flags (e.g. cargo JSON). Parsing consumes the injected/machine stream; user-visible rendered output, `raw_text`, and stored `full` handles are derived from user-relevant output with injected machine records stripped when not required to preserve signals. Parse-miss/fallback must not dump injected machine-format noise.
- **Output:** header `exit <code|timeout>[, <dur>][, <counts/summary>]` where parsed counts are known (e.g. passed/failed/errors/warnings). In `minimal` on clean success, prefer summary-only output (omit redundant bodies/boilerplate) while keeping counts/exit. Failures preserve error/fail/test facts, error-tied context/frames when available, and terminal summaries; compaction is signal-first (on error paths warning-only bodies collapsed/counted). `full: ~h` provided when content is omitted/needed for lossless access.
- **Errors:** `exec_not_found`, `permission_denied`, `policy_denied`, `invalid_args`, `cwd_not_found`.

### 5.9 `check`
- **Args:** `kind=all|build|lint|types|test`, `path="."`, `budget?`.
- **Detection/mapping (v0.1):**
  - Rust (`Cargo.toml`): build/test/clippy via cargo parsers
  - Node (`package.json`): scripts/tools detect tsc/eslint/jest/vitest when configured
  - Python (`pyproject.toml`/`setup.cfg`): pytest/ruff when configured/detected
  - Go (`go.mod`): `go test`/vet/build as applicable
- **Normalization:** to `Diagnostic`/`TestResult`, dedupe `(file,line,col,code,norm_msg)`, sort severity desc then location, flood-group with count+handle.
- **Output:** count-bearing header — `ok[, <P> passed][, <W> warnings]` when no errors/failures, else `<E> errors, <W> warnings[, <F> failed][, <P> passed]` — plus records/summary; signal-first compaction on failures, handles for omitted/verbose detail.
- **Errors:** `no_checker_detected` (actionable hint), `policy_denied`, `outside_workspace`.

### 5.10 `git`
- **Args:** `op` (req) + op args.
- **Read ops (read_only):** `status`, `diff` (`staged=false`, `pathspec?`, `context?`), `log` (`limit=20`, `ref?`), `show` (`ref`, `pathspec?`), `blame` (`path` req, `lines?`), `branch`.
- **Write ops (git_write, explicit):** `commit` (`message` req, `all=false`, `allow_empty=false`), `push` (`remote?`, `branch?`, `set_upstream=false`). v0.1 MUST NOT support force push/amend-by-default destructive flags.
- **Behavior:** native `gix` where supported; compact deterministic rendering (grouped status, minimal hunk headers, one-line log). If operation unsupported by backend, `!git_error unsupported op '<op>' in v0.1 backend. <hint>`.
- **Errors:** `not_a_repo`, `git_error`, `policy_denied`, `invalid_args`.

### 5.11 `expand`
- **Args:** `ref` (req), `grep?` (regex), `lines?`, `head?`, `tail?`, `offset=0` (line/byte tokenized window per mode), `budget?`.
- **Semantics:** resolve handle in session; apply filters in order `grep` → `lines` → `head|tail` → `offset/cap`. Non-`all` views are capped by `handles.expand_max_tokens` and emit `# remaining=N next_offset=M` when truncated.
- **Errors:** `handle_not_found`, `handle_expired`, `invalid_regex`, `invalid_range`, `invalid_args`.

### 5.12 `batch`
- **Args:** `calls` (req array of `{tool:string,args:object}`), max 16 (config `batch.max_calls`), `budget?`.
- **Execution:** validate envelope and each child schema; compute effective tier as max(child tiers) and policy-check upfront for gated tools; execute children in parallel when independent (v0.1 default all independent); collect results; emit in input order. Child failure MUST NOT cancel siblings.
- **Budgeting:** allocate per-child default budgets proportionally within total, but MUST preserve each child’s must-keep items; if total exceeded emit per-child results with required elisions and overall `# budget_exceeded_must_keep` when applicable.
- **Output:** header `<ok> ok, <err> error, <n> total` then blocks `#<i> <tool> ok|error` followed by child payload.
- **Errors:** envelope/schema issues `invalid_args`; child errors inline.

### 5.13 `sh` (escape hatch)
- **Args:** `command` (string, req), `cwd?`, `timeout_ms?`, `budget?`.
- **Policy:** requires tier ≥ execute and `exec.allow_shell=true` (default false). MUST record usage in stats/discover.
- **Execution:** `/bin/sh -lc <command>` on POSIX; same env scrub/limits/capture/redaction as `run`; generic compaction only; full output handle-backed when non-trivial.
- **Errors:** `policy_denied` when gated, else `timeout`, `permission_denied`, `invalid_args`.

---

## 6. Wire format `lean@0.2` (normative)

### 6.1 Grammar (informal)
```text
response   := header LF body?
header     := summary
body       := (line LF)* line?
line       := record | elision | advisory | error | detail
summary    := tool-defined counts/status/exit/duration
elision    := "...+" N SP unit SP "(" handle ")" [SP note]
advisory   := "# " text
error      := "!" code SP message ["." SP hint]
handle     := "~" [a-z0-9]{1,8}
```
- UTF-8, `\n` only, no trailing whitespace, final `\n` required, no ANSI/CSI, no blank separator lines.

### 6.2 Rules
- **Header first:** always line 1.
- **Explicit elision:** omission MUST be represented by `...+<N> more (~<id>)` / `...+<N> lines (~<id>)` / `...+<N> bytes (~<id>)`.
- **Records:** single space field separation; paths workspace-relative (`/`); prefer grouping to avoid path repetition.
- **Indentation:** semantic only (tree depth, nested outline/code blocks in read).
- **Errors/advisories:** may appear after header; must-keep errors MUST appear in body (not only in handle) unless identical and flood-grouped with explicit count + representative.
- **Output verbosity (lean):** `minimal` (default) prioritizes token efficiency on success — emit count-bearing headers and omit redundant success boilerplate (repeated suite banners, non-essential hashes/metadata, low-signal bodies) when signals are preserved; `normal`/`verbose` include more detail. Failures/warnings MUST never be downgraded (I1/I2).
- **Durations:** `run`/exec tools use `Nms` for <10000ms else `N.Ns`.
- **Determinism:** stable ordering per tool; no locale formatting.

### 6.3 JSON mode
```json
{
  "ok": true,
  "tool": "search",
  "meta": {"duration_ms": 12, "counts": {"files": 3, "matches": 7}, "redactions": 0, "notes": []},
  "data": {},
  "diagnostics": [],
  "handles": [{"ref":"~s1","kind":"search","bytes":4096,"hash":"b3:..."}],
  "errors": []
}
```
Stable key order as above at top level; arrays preserve deterministic order.

### 6.4 Golden examples (canonical)
```text
# tree
src/ (214 files, 9 dirs)
  main.rs
  config.rs
  net/ (31)
...+6 more (~t1)

# read unchanged
unchanged src/config.rs (b3:ab12cd34ef567890)

# search
2 files, 3 matches
src/config.rs:41: pub fn parse_config(path: &Path) -> Result<Config>
src/main.rs:12,58: parse_config(&args.path)?

# run fail
exit 1, 4200ms, 212 passed, 2 failed
FAIL tests/auth.rs::login_expired: left=401 right=200 (auth.rs:88)
FAIL tests/auth.rs::refresh_token: timeout 5s (auth.rs:120)
full: ~r7

# error
!ambiguous_anchor 3 matches in src/a.rs at 12:5, 40:5, 88:5. try: w edit path=src/a.rs lines=40:44 replace="..."
```

---

## 7. Algorithms and invariants

### 7.1 Budget fitting (normative)
1. Compute `effective_budget = min(user_budget_or_default, tool_max)`.
2. Partition items into `must_keep`, `high`, `low` with `est_tokens` and `value`.
3. Emit all `must_keep`. If sum > effective_budget, emit all and append `# budget_exceeded_must_keep`.
4. Fill remainder by descending `value/est_tokens`, stable by original order on tie.
5. Replace omitted contiguous/logical groups with one elision + handle containing full omitted content. MUST NOT split/drop any must-keep item.

**Must-keep classes:** summary header; exit/timeout/status; all error and fail records (or explicit counted representatives for identical floods with handle); counts/totals; redaction/security notes affecting interpretation.

### 7.2 Token estimation
- Runtime: deterministic heuristic estimator (calibrated bytes/chars model, no network).
- CI calibration: compare against pinned tokenizer on `corpus/`; fail if MAPE > 10%.

### 7.3 Search ranking and correctness
- Score tuple (desc): definition, exact-case, ledger proximity, recency bucket, source weight, match quality. Tie-break: `path,line,col`.
- Correctness over speed: verify candidates; on index issues use scan fallback with advisory.

### 7.4 Read diffing and ledger verification
- On read: hash current scope; compare with `ReadSeen`. Mismatch or expiry invalidates entry. Emit `unchanged` only after successful verification.
- Diff mode: line diff shown→current, render with new-file line numbers; choose diff if smaller than full and within budget.

### 7.5 Generic compaction (`run` fallback / `sh`)
1. Strip ANSI/CSI/control noise and progress rewrites.
2. Normalize whitespace runs without merging tokens/identifiers.
3. Fold repeats differing only by numbers/hashes/timestamps/templates with counts; strip common toolchain frames to concise markers.
4. **Signal-first on errors/failures:** preserve exit/timeout facts, error/fail/panic/assert/test-failure signals and identifiers, error-tied context/frames when available, and terminal summaries (`could not compile`, `error: aborting...`, `test result: FAILED...`). On error paths, warning-only bodies/context are collapsed/counted (not emitted as blocks); warning lines are not treated as error anchors. In warning-only or clean runs, preserve compact diagnostics/counts/summaries.
5. Compose compact selected windows under budget with explicit elisions; store user-relevant full raw in a handle when omitted/needed for lossless access. Parse-miss/fallback must not emit injected machine-format records.

### 7.6 Diagnostic normalization
- Map to `Diagnostic/TestResult`; normalize paths relative root.
- Dedupe key `(file,line,col,code,normalized_message)`.
- Sort severity desc (`error>warn>info`) then `file,line,col,code`.
- Flood: >20 identical `(tool,code,message)` across locations → `N occurrences (...) last: ...` + handle.

### 7.7 Invariants (MUST be tested)
- **I1 Must-keep preservation:** no dropped raw error/fail/exit/timeout signals without explicit count.
- **I2 Lossless reference:** every elision has resolvable handle with exact omitted payload within TTL/cap.
- **I3 Determinism:** byte-stable output for fixed inputs/state.
- **I4 Verified state claims:** `unchanged`/diff-base only after hash verification.
- **I5 Confinement:** path jail and tier/gate enforcement hold for all tools/args.
- **I6 Schema budget:** exposed MCP schema ≤1500 tokens.
- **I7 Handle integrity:** expand returns exact stored bytes (post-filter semantics defined).
- **I8 Reference reuse:** when identical verified scope is re-requested in-session, prefer `unchanged`/handle reference over re-emitting full body (ledger-backed).

---

## 8. Security and policy

### 8.1 Threat model
- **Untrusted:** workspace contents, command/tool output, agent intent (prompt injection).
- **Trusted (when approved):** user config, explicitly trusted project config, CLI flags from operator context.

### 8.2 Capability tiers and gates
`read_only(0) < write_workspace(1) < execute(2) < git_write(3)`; `allow_shell` boolean gate for `sh` (default false). Default session tier `write_workspace` (configurable). Dispatcher MUST deny if `session.tier < tool.min_tier` or gate off → `!policy_denied required=<...> have=<...>`.

### 8.3 Path jail
- Canonicalize real paths (resolve symlinks, normalize `..`); target MUST be within root or exact allowlisted path (`policy.path_allowlist`). Deny on escape, null bytes, or unsupported path forms → `!outside_workspace`.

### 8.4 Execution controls
- `run`: argv exec, no shell; `sh`: gated shell. Cwd jailed. PTY only when necessary (v0.1: none/minimal).
- Env: base allowlist `PATH,HOME,LANG,LC_ALL,TERM,COLORTERM` plus config `exec.env_allow`; per-call `env` keys permitted only if in effective allowlist and not secret-denylisted; strip values for keys matching secret patterns from child env by default.
- Limits: wall timeout (TERM grace then KILL process group), `exec.max_output_bytes` stream cap, bounded in-memory tail, max parallelism.

### 8.5 Redaction (best-effort, default on)
- Applied before emit, handle storage, stats payloads, audit details.
- Patterns (v0.1 baseline): private key blocks, AWS access key IDs, common token prefixes, `(?i)(api[_-]?key|secret|token|password|passwd|pwd)\s*[:=]\s*['"]?\S+`, URL credentials `://user:pass@`.
- High-entropy heuristic MAY be used only in sensitive contexts to limit false positives (configurable). Redactions counted in `meta.redactions`; MUST NOT leak raw values.

### 8.6 Audit and privacy
- Audit log optional (`telemetry.audit_log`, default false): JSONL `{ts_ms,session,tool,tier,decision,ok,code?,redactions,argv_hash?,paths?}` with no secret/raw output values.
- Stats local-only (`telemetry.local_stats` default true): aggregate token/call counts. `telemetry.network` MUST default false and v0.1 MUST NOT implement network emission.

---

## 9. Configuration (schema v1)

```toml
schema_version = 1

[session]
default_tier = "write_workspace"   # read_only|write_workspace|execute|git_write
ledger_ttl_secs = 7200
serialize_requests = true

[handles]
ttl_secs = 86400
max_bytes = 524288000
expand_max_tokens = 8000

[exec]
timeout_ms = 120000
max_timeout_ms = 600000
max_output_bytes = 10485760
allow_shell = false
env_allow = ["PATH","HOME","LANG","LC_ALL","TERM","CARGO_*", "..."] # toolchain-aware patterns; secrets still denied
kill_grace_ms = 2000

[read]
head_lines = 40
max_full_lines = 400

[search]
default_limit = 50
max_limit = 200
context_max = 20

[edit]
post_check = true

[batch]
max_calls = 16

[output]
passthrough_tokens = 160           # small unstructured body threshold
min_gain_pct = 15                  # require minimum token gain vs passthrough
verbosity = "minimal"              # minimal|normal|verbose (lean output detail)
edit_confirm = "compact"           # compact|detailed

[index]
enabled = true
languages = ["rust","typescript","tsx","python","go"]
max_tree_entries = 20000
use_trigrams = false               # v0.1 MAY ignore/false

[telemetry]
local_stats = true
audit_log = false
network = false                    # MUST remain false in v0.1

[policy]
path_allowlist = []                # absolute canonical paths outside root
```
**Validation:** unknown keys → warning in `w config show` and ignored (forward-compatible) unless `strict_config=true` (optional). Invalid values/types → startup/CLI error (exit 2).

---

## 10. Parsers and extensibility

### 10.1 Declarative TOML parser (v0.1)
```toml
schema_version = 1
name = "cargo-test-json"
priority = 100

[match]
argv0 = "cargo"
argv_contains = ["test"]            # all must be present in order-insensitive subset as specified

[inject]
args = ["--message-format=json", "-q"]  # appended iff not conflicting (implementation-defined safe set)

[parse]
format = "jsonl"                   # json|jsonl|lines|junit
success_exit = [0]

[map.test_result]
id = "$.name"
status = "$.event"                 # transformed by status_map
message = "$.stdout"
location = "$.executable"          # or file:line if available

[map.status]
ok = "pass"
failed = "fail"
ignored = "skip"
```
- Selection: highest `priority` among matching enabled parsers; deterministic tie-break by `name`.
- On parse error: emit `# parse_fallback <reason>` and return generic-compacted output with raw handle (MUST NOT discard raw).

### 10.2 v0.1 built-in parsers
`cargo-build`, `cargo-test`, `cargo-clippy`, `go-test`, `pytest`, `jest`, `vitest`, `tsc`, `eslint`, `ruff` (as detected). Generic fallback always available.

### 10.3 Rules/overrides
- User: `~/.config/wxec/rules.d/*.toml`; Project: `.wxec/rules.d/*.toml` (trusted only). Same precedence as config; project requires trust.

---

## 11. Testing, CI, and benchmark plan

### 11.1 CI jobs (required)
1. `fmt`/`clippy`/`build` (all targets).
2. `unit` + `golden` (wire/JSON/schema snapshots).
3. `invariants` + `property` (I1–I7).
4. `differential` (bash/grep/git parity fixtures).
5. `fuzz` (scheduled + smoke on PR): args, paths, parser inputs, formatter.
6. `schema-budget` (G5) and `estimator-calibration` (G9).
7. `perf-smoke` (G6–G8 thresholds, tolerated variance documented).
8. `security` (path-jail matrix, tier gates, redaction fixtures).

### 11.2 Repository test layout
```text
crates/*/tests/...
tests/golden/{tree,read,search,edit,run,check,git,expand,batch,sh,errors}/*.lean
tests/fixtures/{repos,outputs,parsers}/...
corpus/{raw,scrubbed,expected}/...
bench/tasks/*.yaml
bench/baselines/...
```

### 11.3 Benchmark harness
- Tasks: fix failing test, locate regression cause, rename symbol, explain module, resolve lint/type errors, summarize git diff, etc. (≥30).
- Runners: plain bash, `wxec` MCP/CLI, and (where supported) RTK/other baselines with **fair command mapping** per task/harness (no mismatched unsupported subcommands).
- Metrics: success (pass criteria per task), turns, tool calls, input/output/total tokens, wall time, variance. Report deltas and gate pass/fail.
- **Agentic E2E (required for “beats X” claims):** pinned model/agent harness, fixed task set, record turns/retries and model tokens incl. prompts/tool schemas where measurable; compare `w` vs baselines on the same protocol. Scripted CLI benchmarks are necessary but not sufficient for agentic token claims.

---

## 12. Implementation milestones (v0.1)

| M | Deliverable | Exit criteria |
|---|---|---|
| M0 | Repo scaffold, CI, config/schema loader, wire formatter skeleton, golden harness | empty-tool roundtrip + G5 check scaffolding |
| M1 | `wxec-core` (dispatcher, session, policy, budget, estimator, handles) + `tree/read/search/expand` + workspace index + MCP stdio + CLI | I2–I4,I6 pass for scope; nav benchmark shows token win |
| M2 | executor + `run` + parsers (10) + `check` | differential/exit parity; I1 on exec outputs |
| M3 | `edit` + `git` read/write-gated + redaction + audit | I1,I5 security suites pass |
| M4 | ledger diffing/status diffs + `batch` + `sh` + `gain/discover` | end-to-end tasks runnable; perf smoke within targets |
| M5 | fuzz hardening, docs (`docs/wire-format.md`, `docs/tools.md`, `docs/parsers.md`, `docs/security.md`), packaging (brew/cargo/dist binaries), benchmark report | **G1–G10 pass → v0.1.0** |

---

## 13. Operational commands and artifacts
- `w --version`, `w --help`, `w <tool> --help` (schema-derived).
- `w config show` prints effective config with redacted sensitive values.
- `w handles gc` enforces TTL/cap immediately.
- Logs/traces: controlled by `WXEC_LOG` (e.g. `error,warn,info,debug,trace`), redacted, local stderr/file as configured (default off/minimal).

---

## 14. Resolved decisions (v0.1, final for implementation)

| Decision | Resolution |
|---|---|
| Product/CLI/MCP naming | `w bash` / `w` / `wxec` / `wxec-*` |
| License | MIT |
| Primary transport | MCP stdio (v0.1); HTTP deferred |
| Platforms | Linux/macOS; Windows via WSL |
| Symbol languages | Rust, TypeScript/TSX, Python, Go |
| Edit semantics | literal anchor/line/create/delete; atomic; `expect_hash`; no patch input |
| Parsers | TOML declarative v1; WASM v2 |
| Shell | `run` argv-only; `sh` gated off by default |
| Telemetry | local-only; `network=false` and unimplemented in v0.1 |
| Session concurrency | serialized by default; parallel within `batch` allowed |
| Error/output safety | must-keep preservation, explicit elision, redact-before-store/emit |

---

## 15. Implementation kickoff checklist (ready-to-execute)

- [ ] Create workspace/repo `wxec/` with crate skeleton and CI per §11.1, §12 M0.
- [ ] Commit this spec as `docs/spec-v3.md` and wire format as `docs/wire-format-v0.2.md` (derived, consistent).
- [ ] Define MCP/CLI JSON schemas for all tools and add schema-token gate test (G5).
- [ ] Implement `wxec-core` types, canonicalization, formatter, budget fitter, and golden tests for §6.4 examples.
- [ ] Add corpus fixtures and baseline harness before optimization work (G1–G3 measurement baseline).
- [ ] Implement policy/path jail/redaction primitives before `run/edit/git-write` enablement.
- [ ] Track all gate metrics in CI artifacts; block release on any regression past thresholds.

**This specification is complete and approved for v0.1 implementation pending no external constraint conflicts; deviations during implementation MUST be recorded as spec amendments with version bump and updated gates/tests.**
