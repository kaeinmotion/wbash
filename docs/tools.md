# wxec tools reference manual

complete parameter specifications, execution tiers, and output behaviors for all wxec model context protocol (mcp) tools.

---

### **table of contents**

- [overview & global parameters](#overview--global-parameters)
- [read-only inspection tools](#read-only-inspection-tools)
  - [`tree`](#tree)
  - [`read`](#read)
  - [`search`](#search)
  - [`expand`](#expand)
- [workspace editing tools](#workspace-editing-tools)
  - [`edit`](#edit)
  - [`replace`](#replace)
- [execution & diagnostics tools](#execution--diagnostics-tools)
  - [`check`](#check)
  - [`run`](#run)
  - [`sh`](#sh)
- [version control tools](#version-control-tools)
  - [`git`](#git)
- [orchestration tools](#orchestration-tools)
  - [`batch`](#batch)
- [error catalog & diagnostics](#error-catalog--diagnostics)
- [system exit codes](#system-exit-codes)

---

### **overview & global parameters**

all tools operate under strict token budgets and output minimization heuristics. all tool calls accept the following optional runtime control parameters:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `format` | string | `"compact"` | output serialization format (`"compact"` for token-optimized text, `"json"` for structured payload) |
| `budget` | integer | tool-specific | maximum estimated token budget allocated for the response (`64..=32768`) |
| `verbosity` | string | `"minimal"` | output density (`"minimal"`, `"normal"`, or `"verbose"`) |

---

### **read-only inspection tools**

#### **`tree`**
generates an optimized hierarchical view of directory contents. small leaf directories are inlined, while large subtrees are collapsed into lossless memory handles (`~id`).

- **tier required**: `read_only` (level 0)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `path` | string | `"."` | root directory path relative to workspace root |
| `depth` | integer | `2` | maximum traversal depth (`0` = unlimited traversal) |
| `glob` | string | `null` | glob filter to restrict listed entries (e.g. `"*.rs"`) |
| `sizes` | boolean | `false` | include human-readable byte sizes for entries |
| `dirs_only` | boolean | `false` | list only directories and omit regular files |

- **behavior**:
  - collapsed directories format as `name/ (N) ~h` with a handle for drill-down.
  - when `dirs_only=true`, outputs `{display}/ ({dirs} dirs)` without file tokens.

---

#### **`read`**
surgical file and symbol inspector with session ledger verification and boundary context expansion.

- **tier required**: `read_only` (level 0)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `path` | string | *required*\* | relative path to file (supports `#symbol`, `#L10-L20`, `:10-20`, and unique suffix resolution) |
| `paths` | array | `[]` | multiple file paths to inspect concurrently in one turn |
| `lines` | string | `null` | line range (`"A:B"`, `"A-B"`, or single line `"N"`, 1-based inclusive) |
| `context` / `around` | integer | `null` | number of boundary lines to include before and after target line range or symbol |
| `symbol` | string | `null` | target code symbol or function name (auto-extracts doc comments and decorators) |
| `outline` | boolean | `false` | extract high-level code signatures and symbol hierarchies |
| `expect_hash` | string | `null` | expected blake3 digest to prevent reading stale revisions |
| `force` | boolean | `false` | bypass session hash ledger and re-emit full contents |

*\* Either `path` or `paths` is required.*

- **behavior**:
  - **suffix & fuzzy resolution**: if a file is not at the exact path, resolves unique workspace suffix matches (e.g. `path: "fs.rs"` $\rightarrow$ `crates/wxec-tools/src/fs.rs`) without failing.
  - **session hash ledger**: unchanged files return `unchanged <path> (b3:...) expand ~h` (cost: 27 tokens).
  - **budget capping**: large files emit the head range, an elision marker `...+N lines (~h)`, and symbol outlines.

---

#### **`search`**
simd-accelerated workspace search supporting plain text, regular expressions, and code symbols.

- **tier required**: `read_only` (level 0)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `pattern` / `query` | string | *required* | search string, regex pattern, or symbol name |
| `path` | string | `"."` | directory or file scope to search |
| `paths` | array | `[]` | multiple file or directory paths to search simultaneously |
| `mode` | string | `"text"` | search algorithm: `"text"` (literal), `"regex"`, or `"symbol"` |
| `glob` | string | `null` | file pattern filter (e.g. `"*.toml"`) |
| `context` | integer | `0` | number of leading and trailing context lines |
| `limit` | integer | `null` | maximum number of matches to return |
| `files_only` | boolean | `false` | return only unique file paths with matches, short-circuiting line scans |
| `case_insensitive` | boolean | `false` | case-insensitive pattern matching |
| `word` | boolean | `false` | match whole words only (word boundary matching) |
| `stats` | boolean | `false` | append execution footer with scanned file counts |

- **behavior**:
  - uses vectorized memory pre-matching (`memmem`).
  - safe fallback: in literal mode, unescaped `a|b` patterns auto-retry alternation safely.
  - groups matches by file when a single file yields 3 or more occurrences.

---

#### **`expand`**
retrieves cached data from an atomic handle (`~id`) generated by truncated outputs, large diffs, or build logs.

- **tier required**: `read_only` (level 0)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `id` / `ref` | string | `null` | single handle identifier (e.g. `"~h101"` or `"~s2"`) |
| `refs` / `handles` | array | `[]` | multiple handle identifiers (`["~h1", "~h2"]`) to expand simultaneously |
| `grep` | string | `null` | regex filter applied to handle contents |
| `lines` | string | `null` | line range slice (`"A:B"` or `"A-B"`, 1-based) |
| `head` | integer | `null` | extract first n lines from handle |
| `tail` | integer | `null` | extract last n lines from handle |
| `offset` | integer | `0` | byte or line offset for pagination |
| `all` | boolean | `false` | stream entire handle without token budget capping |
| `depth` | integer | `0` | inline nested child handles up to specified depth |

---

### **workspace editing tools**

#### **`edit`**
surgical code editor utilizing exact literal matching, line-range splices, or file lifecycle operations.

- **tier required**: `write_workspace` (level 1)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `path` | string | *required* | relative path (supports `#symbol`, `#L10-L20`, and unique suffix auto-resolution) |
| `find` / `target` | string | `null` | exact literal string to be replaced (accepts `~id` or `@~id` handle references) |
| `replace` / `replacement` | string | `null` | drop-in replacement content (accepts `~id` or `@~id` handle references) |
| `lines` | string | `null` | line range to splice (`"A:B"` or `"A-B"`) |
| `content` | string | `null` | replacement or initial content (accepts `~id` or `@~id` handle references) |
| `occurrence` | integer | `null` | target specific match index (`1` for first, `-1` for last) |
| `near_line` | integer | `null` | target match nearest to specified line number |
| `all` | boolean | `false` | replace all occurrences across target file |
| `create` | boolean | `false` | create new file only; fails if file already exists (unless overwrite=true) |
| `overwrite` | boolean | `false` | explicit overwrite flag (optional when content is passed to update existing file) |
| `delete` | boolean | `false` | delete specified file |
| `append` | boolean | `false` | append content to the end of the file |
| `prepend` | boolean | `false` | prepend content to the beginning of the file |
| `expect_hash` | string | `null` | reject edit if current file hash does not match |

- **behavior**:
  - atomic file updates via temporary file replacement.
  - auto-creates missing parent directories.
  - normalizes crlf and lf line endings automatically.
  - returns compact confirmation: `edited <path> (+ins -del)`.

---

#### **`replace`**
global pattern replacement spanning multiple workspace files with dry-run verification.

- **tier required**: `write_workspace` (level 1)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `find` | string | *required* | pattern to find across files (accepts `~id` handle references) |
| `replace` | string | `""` | replacement string (accepts `~id` or `@~id` handle references) |
| `path` | string | `"."` | directory or path scope (supports unique suffix auto-resolution) |
| `paths` | array | `[]` | multiple directory or path scopes |
| `glob` | string | `null` | file pattern filter |
| `word` | boolean | `false` | match whole words only |
| `regex` | boolean | `false` | interpret `find` as regular expression |
| `dry_run` | boolean | `false` | simulate replacements without modifying disk |
| `max_files` | integer | `200` | maximum files allowed to be modified in one call |
| `limit` | integer | `null` | maximum match replacements per file |

---

### **execution & diagnostics tools**

#### **`check`**
intelligent project compiler and diagnostic runner with automatic build system detection.

- **tier required**: `execute` (level 2)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `kind` | string | `"all"` | verification scope: `"test"`, `"build"` (alias: `compile`), `"lint"` (aliases: `clippy`, `fmt`), `"type"`, or `"all"` |
| `path` | string | `"."` | sub-package or project root directory |
| `args` / `filter` / `test` | array \| string | `null` | selective test filter or runner args (e.g. `"test_name"`, routed strictly to test runner) |

- **behavior**:
  - auto-detects cargo (rust), npm/pnpm/yarn/bun (javascript/typescript), pytest/ruff (python), and go test (go).
  - parses compiler errors into structured diagnostics and isolates test failure lines.
  - compresses passing test noise into compact summaries (`ok, N passed`).

---

#### **`run`**
secure, argv-structured process runner with output stream parsers.

- **tier required**: `execute` (level 2)
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `argv` / `cmd` | array \| string | *required* | executable and arguments (array preferred; string auto-tokenized) |
| `cwd` | string | `"."` | working directory (jailed to workspace) |
| `timeout_ms` | integer | `120000` | execution timeout in milliseconds |
| `stdin` | string | `null` | input string piped to standard input |
| `env` | object | `{}` | allowlisted environment variable overrides |

---

#### **`sh`**
shell execution fallback for complex pipelines. disabled by default for security.

- **tier required**: `execute` (level 2) and `exec.allow_shell=true`
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `command` | string | *required* | shell command string to evaluate |
| `cwd` | string | `"."` | working directory |
| `timeout_ms` | integer | `120000` | timeout in milliseconds |
| `stdin` | string | `null` | input string piped to standard input |

---

### **version control tools**

#### **`git`**
complete git lifecycle management with structured diff compaction.

- **tier required**: `read_only` for queries, `git_write` (level 3) for mutations
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `op` | string | *required* | operation: `"status"`, `"diff"`, `"log"`, `"show"`, `"blame"`, `"branch"`, `"checkout"`, `"switch"`, `"commit"`, `"push"`, `"pull"`, `"tag"`, `"tags"`, `"remote"`, `"custom"` |
| `pathspec` / `path` | string | `null` | file path filter or tag name |
| `staged` / `cached` | boolean | `false` | view staged changes in diff |
| `context` | integer | `3` | unified diff context lines (`-U <n>`) |
| `stat` | boolean | `false` | include diffstat summary |
| `msg` / `message` | string | `null` | commit message or annotated tag message |
| `all` / `auto_stage` | boolean | `false` | automatically stage tracked modified files during commit |
| `limit` | integer | `null` | limit log commit entries or blame lines |
| `remote` | string | `null` | git remote name (default: `"origin"`) |
| `branch` | string | `null` | git branch name |
| `tags` | boolean | `false` | include `--tags` when running `push` |
| `rebase` | boolean | `false` | use `--rebase` when running `pull` |
| `args` | array | `null` | explicit argument list when using `op="custom"` |

---

### **orchestration tools**

#### **`batch`**
executes up to 16 heterogeneous tool calls in a single round-trip turn.

- **tier required**: evaluated per individual call
- **parameters**:

| parameter | type | default | description |
| :--- | :---: | :---: | :--- |
| `calls` | array | *required* | list of call objects: `[{"tool": "name", "args": {...}}]` (aliases: `params`, `parameters`, `input`) |
| `stop_on_error` | boolean | `false` | halt subsequent calls immediately if any step fails |
| `compact` | boolean | `false` | emit a consolidated Net Digest rollup (`batch rollup: N ok (+ins -del, N passed)`) |

- **behavior**:
  - child results are delimited: `#1 tool ok`, `#2 tool error`.
  - nested `batch` calls are strictly rejected.

---

### **error catalog & diagnostics**

every error emitted by wxec includes an actionable recovery hint:

| error code | root cause | actionable recovery hint |
| :--- | :--- | :--- |
| `outside_workspace` | path escapes configured workspace root | adjust path to reside inside workspace or allow via `WXEC_ALLOW_PATHS` / `policy.path_allowlist` |
| `ambiguous_anchor` | `find` string matches multiple locations | specify line numbers via `lines` or expand anchor context |
| `anchor_not_found` | `find` string was not found in target file | verify file contents via `read` before editing |
| `hash_mismatch` | file was modified concurrently | fetch latest version with `read force=true` |
| `line_range` | requested lines out of file bounds | check line count and supply valid 1-based range |
| `policy_denied` | tool requires higher permission tier | update server tier configuration (`--tier`) |
| `handle_not_found` | handle id does not exist or expired | re-run source command to generate a fresh handle |
| `timeout` | command exceeded timeout limit | increase `timeout_ms` or optimize command |
| `invalid_args` | missing or unrecognized parameter | consult parameter schema in this document |

---

### **system exit codes**

- `0`: clean success.
- `1`: domain failure (e.g. failing unit tests or lint errors reported by `check`).
- `2`: argument validation, policy restriction, or configuration error.
- `124`: child process timeout under `run` or `sh`.
- `70`: internal runtime error.
