# Tools

All tools accept common `format`/`budget` arguments (CLI: `--format`, `--budget`, `--verbosity minimal|normal|verbose`).
JSON schemas are generated from the same argument structs and exposed over MCP
`tools/list`; unknown fields are rejected.

| Tool | Tier | Purpose |
|---|---|---|
| `tree` | read_only | Compact directory tree with counts |
| `read` | read_only | File/range/symbol read with unchanged/diff verification |
| `search` | read_only | Text or regex search, definition-ranked |
| `expand` | read_only | Expand a handle with grep/lines/head/tail filters |
| `edit` | write_workspace | Literal anchor edit, line splice, create, delete |
| `replace` | write_workspace | Multi-file text replace (literal/word/regex), dry-run |
| `run` | execute | argv execution with parsers and compact fallback |
| `check` | execute | Detect and run build/lint/type/test commands |
| `git` | read_only (writes gated) | status/diff/log/show/blame/branch/commit/push |
| `batch` | per-call | Serialized multi-call with per-call failures inline |
| `sh` | execute + `allow_shell` | Shell command with generic compaction |

## Arguments

- `tree`: `path="."`, `depth=2` (0 = unlimited), `glob`, `sizes=false`, `dirs_only=false`.
  Collapsed directories render as `name/ (N) ~h`; small leaf directories at a
  depth boundary may be shown inline. Large file groups become `~h` handles. When `dirs_only=true`, omits regular files and formats header as `{display}/ ({dirs} dirs)`.
- `read`: `path`, optional `lines="A:B"` (1-based, clamped), `symbol`,
  `outline=false`, `expect_hash`, `force=false`. Repeated unchanged reads return
  `unchanged <path> (b3:...) expand ~h`; `force=true` re-emits content. Large
  files emit head, `...+N lines (~h)`, then outline symbols beyond the head
  (capped) and `+N more symbols (~h)` when needed.
- `search`: `pattern`, `mode="text"|"regex"|"symbol"`, `path="."` (or multiple paths), `paths`, `glob`, `context=0`,
  `limit`, `stats=false`. Returns ranked matches; CLI accepts multiple positional paths and
  `-e/--regexp <pattern>` (e.g. `w search -e pat src tests`). When `stats=true`, appends footer `# stats: scanned N files, M matches`. Text mode retries
  safe `a|b` alternation on zero literal matches (`# '|' alternation`) or notes
  `# '|' literal; try mode=regex`. Repeated identical searches may return
  `unchanged search (N files, M matches)`. Counts use singular/plural forms;
  zero-result searches that skipped binary files note
  `# skipped N binary file(s)`. Files with >=3 returned hits are grouped as
  `path (N):` with indented `line: snippet`.
- `expand`: `ref="~id"`, `grep`, `lines`, `head`, `tail`, `offset=0`, `all=false`, `depth=0`. Non-`all` views are capped by `handles.expand_max_tokens` and emit `# remaining=N next_offset=M` when truncated. `all=true` emits full selected content (capped by its internal limits); `depth` inlines nested text handles.
- `edit`: `path` plus exactly one operation: `find`+`replace` (unique literal
  match), `lines`+`content` (splice), `create=true`+`content`, `overwrite=true`
  + `content`, or `delete=true`. `expect_hash` rejects stale writes. Writes are
  atomic; a post-write syntax check is advisory. Confirmation is compact by default (`output.edit_confirm`).
- `replace`: `find` (req), `replace` (req), `path="."` (or `paths`), `glob`,
  `word=false`, `regex=false`, `dry_run=false`, `max_files=200`, `limit`. Literal
  by default; replaces all matches in scope. Returns per-file match counts;
  `--verbosity verbose` emits diff lines (budget-fitted). Writes are atomic per file.
- `run`: `argv=[...]` (no shell), `cwd="."`, `timeout_ms`, `stdin`, `env`
  (allowlisted keys only). CLI parses only wxec control flags for `run`
  (`cwd`, `timeout_ms`, `stdin`, `env`); pass child flags after `--` when
  ambiguous. Parsers consume injected/machine streams when used, while rendered
  tails/`raw_text`/`full` handles use user-relevant output with injected
  machine records stripped. Headers include parsed counts when known
  (e.g. passed/warnings/errors); failures preserve error/test signals and
  terminal summaries with compact tails and a `full: ~id` handle as needed.
- `check`: `kind="all"|"build"|"lint"|"type"|"test"`, `path="."`. Runs detected
  project commands (Cargo, npm scripts, pytest, go, ruff, tsc, etc.) and
  reports count-bearing headers (`ok, N warnings, N passed` / errors/failures).
- `git`: `op` (`status`, `diff`, `log`, `show`, `blame`, `branch`, `commit`,
  `push`) plus op fields: `pathspec`, `staged`/`cached`, `context` (`-U`,
  `--unified`), `stat`, `name_only`, `name_status`, `numstat`, `shortstat`,
  `limit`, `ref`, `path`, `lines`, `message`, `all`, `allow_empty`, `remote`,
  `branch`, `set_upstream`. `show` returns compact commit/file/hunk/change
  summary; `blame` is capped by `limit` with omitted lines in a handle.
  `commit` requires `git_write`; `push` requires `git_write`. `log` accepts
  `-n <count>`, `-n<count>`, `--limit <count>` and display-only
  `--oneline`/`--graph`/`--decorate`/`--no-pager`; other short flags are
  rejected with a hint.
- `batch`: `calls=[{tool, args}]`. Calls run serially; child results are
  delimited (`#N tool ok|error`) when they include output or handles and
  failures do not abort siblings. Nested `batch` is rejected.
- `sh`: `command`, `cwd`, `timeout_ms`, `stdin`. CLI parses only wxec control
  flags for `sh` (`cwd`, `timeout_ms`, `stdin`); prefer `--` when command
  text/args are ambiguous. Requires `execute` tier and `exec.allow_shell=true`.

## Errors

Codes include `invalid_args`, `not_found`, `outside_workspace`,
`path_is_dir`, `ambiguous_anchor`, `anchor_not_found`, `hash_mismatch`,
`line_range`, `policy_denied`, `handle_not_found`,
`handle_expired`, `timeout`, `spawn_failed`, `git_failed`, `parse_failed`,
`binary_file`, `too_large`,
`file_exists`, `unknown_tool`, `batch_nested`, `not_trusted`.

Each error message includes a concrete next action (for example a suggested
`w read` range or `w edit` line splice).

## Exit codes

- `0`: success.
- `1`: tool/domain failure (`check` errors, etc.).
- `run`/`sh`: the child exit code when nonzero (timeout: `124`).
- `2`: invocation/validation/policy/config error.
- `70`: internal error.
