### **wxec**

token-efficient execution layer and model context protocol (mcp) server for ai coding agents.

`wxec` is an execution engine designed specifically for ai coding agents. standard shell and native cli tools dump raw build traces, hundred-line test logs, full-file dumps, and repetitive terminal boilerplate into the language model's context window. this noise rapidly saturates context limits, degrades attention quality (the "lost in the middle" problem), inflates api inference costs, and triggers constant ide permission dialogues.

`wxec` operates as a high-density protocol layer between the ai agent and the workspace. built from the ground up in pure rust with zero external binary dependencies, it provides structured, value-ranked tool outputs, transactional session state tracking, deterministic deduplication, and an off-context memory store. by replacing raw terminal dumps with semantic diffs, symbol outlines, and compact handle pointers, `wxec` eliminates up to 84% of tool output tokens while preserving 100% lossless access to full data on demand.

### **table of contents**

- [chapter 1: the token crisis in ai coding agents](#chapter-1-the-token-crisis-in-ai-coding-agents)
- [chapter 2: core architecture & engine design](#chapter-2-core-architecture--engine-design)
- [chapter 3: empirical benchmark results](#chapter-3-empirical-benchmark-results)
- [chapter 4: complete mcp tool catalog & api reference](#chapter-4-complete-mcp-tool-catalog--api-reference)
- [chapter 5: multi-tool batching & turn elimination](#chapter-5-multi-tool-batching--turn-elimination)
- [chapter 6: security policy, sandboxing & permission tiers](#chapter-6-security-policy-sandboxing--permission-tiers)
- [chapter 7: installation & host client integration](#chapter-7-installation--host-client-integration)
- [chapter 8: configuration reference (wxec.toml)](#chapter-8-configuration-reference-wxectoml)
- [chapter 9: error handling & troubleshooting](#chapter-9-error-handling--troubleshooting)
- [chapter 10: license & contribution](#chapter-10-license--contribution)

### **chapter 1: the token crisis in ai coding agents**

ai coding assistants face three fundamental bottlenecks when interacting with workspaces through raw shell interfaces:

1. **terminal verbosity and noise ratio**: a single `cargo test`, `npm test`, or `git diff` can easily generate 1,000 to 5,000 tokens of boilerplate, passing test assertions, and build warnings. the agent only requires the failing assertion, file path, and line number (typically less than 40 tokens of actionable signal).
2. **context window exhaustion and attention dilution**: as raw terminal dumps accumulate across multiple turns, context windows fill up. transformer attention mechanisms suffer from attention dilution, causing models to hallucinate file contents, forget system instructions, or fail to anchor edits cleanly.
3. **ide permission friction**: raw shell tools trigger repetitive approval popups from ide security sandboxes on every modified command shape, stalling autonomous agent execution.

`wxec` solves this crisis by shifting the paradigm from raw text passthrough to structured, token-bounded execution.

### **chapter 2: core architecture & engine design**

`wxec` achieves token compression through four foundational architectural pillars:

#### **the session hash ledger**
every file read and search query is recorded in a transactional session ledger. when an agent inspects a file that has not changed since the last turn, `wxec` returns a verification acknowledgment (`unchanged <path> (<hash>) expand ~<id>`) instead of re-emitting hundreds of lines. the ledger tracks file content hashes, modification timestamps, and query parameters with dirty-state memory caching, completely bypassing disk serialization when state is unmodified.

#### **the lossless handle store (`~<id>`)**
compact outputs must never result in permanent data loss. whenever `wxec` truncates or summarizes a large file, compiler log, or test trace, it preserves the full, raw payload in an off-context atomic blob store identified by an invariant handle (e.g. `~h3f`). the agent can retrieve any slice, line range, or full text at any time using the `expand` tool without polluting its conversation history.

#### **simd-accelerated indexing & workspace cache**
workspace scans bypass expensive filesystem thrashing. `wxec` maintains an ignore-aware in-memory directory tree (`walk_cached_arc`) and performs whole-file pre-filtering with vectorized substring matching (`memmem`). full-repository searches skip non-matching files in milliseconds without executing line-by-line enumeration loops.

#### **calibrated token budget fitting**
all tool results pass through a value-ranked budget fitter. items are classified by utility (`must_keep`, `high`, `low`). if the total output exceeds the target token budget, low-value items are automatically converted to compact tails and handles while preserving critical error diagnostics, line numbers, and actionable hints.

### **chapter 3: empirical benchmark results**

head-to-head testing conducted against standard raw shell commands and alternative proxy tools (rtk 0.37.2) across identical real-world developer tasks, measured with the exact `cl100k_base` tokenizer:

| task | raw shell | rtk (0.37.2) | wxec mcp | token savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| `git status` | 86 tok | 22 tok | **16 tok** | -81.4% |
| `git diff` | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| `read file` (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| `search / grep` | 567 tok | 459 tok | **11 tok** | -98.1% |
| `test execution` | 356 tok | 99 tok | **10 tok** | -97.2% |
| **total benchmark tokens** | **6,257 tok** | **5,381 tok** | **1,018 tok** | **-83.7%** |

#### **turn and latency reduction**
- **total turn count**: reduced from 52 steps in raw shell to 43 steps in `wxec` (-17.3% fewer round-trips).
- **end-to-end turnaround**: cutting 8,600+ generated tokens saves 90 to 140 seconds of pure model generation wait time per complex task.
- **zero dependency failures**: standalone pure rust implementation never fails due to missing system tools (unlike tools that crash if `tree` is not installed on host).

### **chapter 4: complete mcp tool catalog & api reference**

`wxec` exposes an integrated suite of mcp tools covering the full software development lifecycle:

#### **`read`**
token-compact file inspection with symbol outline extraction and unchanged verification.
- `path`: target file path (required).
- `lines`: line range (`"1:50"`, `"1-50"`, or `"1..50"`).
- `symbol`: read specific function or type declaration.
- `outline`: generate high-level symbol hierarchy (heuristic or ast-assisted).
- `force`: bypass unchanged ledger deduplication and force re-read.

#### **`edit`**
surgical code editor supporting exact anchor replacement, line splicing, creation, and deletion.
- `path`: target file path (required).
- `find` (alias: `target`, `old_string`): unique literal string to locate.
- `replace` (alias: `replacement`, `new_string`): new replacement string.
- `lines`: line range to splice.
- `create`: create new file with content.
- `overwrite`: replace entire file content.
- `delete`: remove file.

#### **`search`**
vectorized, definition-ranked workspace search.
- `pattern` (alias: `query`): search string or regular expression.
- `mode`: `"text"`, `"regex"`, or `"symbol"`.
- `path`: root search directory or target file path.
- `glob`: optional file pattern filter.
- `context`: surrounding lines of context (0 to 20).
- `limit`: maximum number of hits to return.

#### **`replace`**
multi-file global pattern replacement with dry-run support.
- `find`: pattern to find across files.
- `replace`: replacement content.
- `path`: root path for replacement scope.
- `glob`: file glob filter.
- `dry_run`: preview replacements without mutating files.

#### **`tree`**
deterministic directory tree with entry counts and bounded depth.
- `path`: directory path (defaults to `"."`).
- `depth`: directory traversal depth limit.
- `sizes`: display file sizes.
- `glob`: file filter pattern.

#### **`check`**
zero-friction compiler and project test suite runner with structured error extraction.
- `kind`: check kind (`"all"`, `"build"`, `"lint"`, `"type"`, `"test"`).
- `path`: workspace directory to check.

#### **`git`**
complete git lifecycle management without shell escaping.
- `op` (alias: `operation`, `command`): operation (`"status"`, `"add"`, `"diff"`, `"commit"`, `"push"`, `"log"`, `"show"`, `"blame"`, `"branch"`).
- `path`: target path or pathspec.
- `message` (alias: `msg`): commit message.
- `staged`: restrict diff or commit to staged changes.
- `all`: stage all tracked and untracked changes (`-A`).

#### **`batch`**
atomic execution of up to 16 tool calls within a single round-trip turn.
- `calls`: array of tool call objects (`[{"tool": "read", "args": {...}}]`).
- aliases: accepts `"name"` for tool and `"arguments"` for args.

#### **`expand`**
on-demand lossless retrieval of omitted text from handle references.
- `id`: handle reference (e.g. `"~3ho"`).
- `lines`: line slice from handle payload.
- `grep`: filter handle lines by regex.
- `head` / `tail`: retrieve start or end slices.

#### **`run`**
structured executable runner with argument injection, child sandboxing, and output compaction.
- `argv` (alias: `cmd`, `args`): process arguments vector (`["cargo", "test"]`).
- `cwd` (alias: `dir`): working directory.
- `timeout_ms`: hard execution timeout.

#### **`sh`**
safe shell execution fallback with output compaction.
- `command`: shell command string.
- `cwd`: working directory.
- `timeout_ms`: execution timeout.

### **chapter 5: multi-tool batching & turn elimination**

one of the largest sources of agent latency is sequential round-tripping. for example, diagnosing and fixing a test failure traditionally takes 4 to 6 separate turns:

1. turn 1: run test suite
2. turn 2: read failing file
3. turn 3: edit file fix
4. turn 4: re-run test suite
5. turn 5: check git status

with `wxec`'s `batch` tool, an agent can bundle related actions into a single round trip:

```json
{
  "calls": [
    {
      "tool": "edit",
      "args": {
        "path": "crates/core/src/parser.rs",
        "find": "let max = 10;",
        "replace": "let max = 20;"
      }
    },
    {
      "tool": "check",
      "args": {"kind": "test"}
    },
    {
      "tool": "git",
      "args": {"op": "status"}
    }
  ]
}
```

child failures in `batch` do not abort sibling calls, and outputs are individually formatted and budget-fitted with clear numbered headers (`#1 edit ok`, `#2 check ok`).

### **chapter 6: security policy, sandboxing & permission tiers**

`wxec` enforces multi-layered defense-in-depth across all workspace interactions:

#### **path confinement & root jailing**
all file paths are canonicalized and strictly jailed within the workspace root. any attempt to escape via path traversal (`../`), symlink redirection, or nul-byte injections is rejected with an `outside_workspace` error before touching disk.

#### **permission tiers**
tools are segregated into strict security tiers:

| tier | level | authorized tools |
| :--- | :---: | :--- |
| `read_only` | 0 | `tree`, `read`, `search`, `expand`, `git` (status/diff/log) |
| `write_workspace` | 1 | `edit`, `replace` |
| `execute` | 2 | `run`, `check`, `sh` |
| `git_write` | 3 | `git add`, `git commit`, `git push` |

#### **automatic secret redaction**
every string rendered to the wire or stored in memory handles is automatically scanned by `Redactor`. aws access keys, bearer tokens, github personal access tokens, private keys, url credentials, and password assignments are masked with `[REDACTED]` using a zero-allocation fast pass.

#### **child process confinement**
child processes run with scrubbed environments (`default_env_allow`). sensitive host environment keys are stripped, while developer essentials (`PATH`, `CARGO_*`, `RUSTFLAGS`, `SSH_AUTH_SOCK`, `XDG_*`) are preserved. processes are governed by hard wall-clock timeouts and process-group kill guarantees.

### **chapter 7: installation & host client integration**

#### **compilation & binary installation**

```sh
git clone https://github.com/kaeinmotion/wxec.git
cd wxec
cargo build --release
cp target/release/wxec ~/.cargo/bin/
cp target/release/w ~/.cargo/bin/
```

#### **antigravity & gemini cli integration**
add `wxec` to your mcp configuration file (e.g. `~/.gemini/antigravity/mcp/wxec/mcp.json` or `~/.gemini/config/mcp_config.json`):

```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": ["--root", "/path/to/your/workspace", "--tier", "git_write"]
    }
  }
}
```

#### **claude desktop & cursor integration**
add to `claude_desktop_config.json` or cursor mcp settings:

```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": ["--root", "/absolute/path/to/workspace"]
    }
  }
}
```

### **chapter 8: configuration reference (wxec.toml)**

`wxec` can be customized globally or per-workspace via `.wxec.toml` or `wxec.toml`:

```toml
[session]
default_tier = "git_write"
ledger_ttl_secs = 86400
serialize_requests = false

[handles]
ttl_secs = 86400
max_bytes = 104857600
expand_max_tokens = 2000

[read]
head_lines = 40
max_full_lines = 400

[search]
default_limit = 50
max_limit = 200
context_max = 20

[exec]
timeout_ms = 120000
max_timeout_ms = 600000
max_output_bytes = 10485760
allow_shell = true
kill_grace_ms = 2000

[output]
passthrough_tokens = 160
min_gain_pct = 15
verbosity = "minimal"
edit_confirm = "compact"
```

### **chapter 9: error handling & troubleshooting**

all `wxec` errors follow structured formats containing error codes, actionable context, and concrete remediation hints:

- `!outside_workspace`: path resolves outside workspace root. use paths relative to repository root.
- `!ambiguous_anchor`: `find` string in `edit` matched multiple locations. provide additional surrounding context or use line-splicing.
- `!anchor_not_found`: `find` string was not located in file. verify line numbers or use fuzzy tolerance.
- `!policy_denied`: requested operation exceeds active session tier. elevate tier in configuration or command flags.
- `!handle_expired`: requested handle reference has timed out. re-run the producing tool to generate a fresh handle.

### **chapter 10: license & contribution**

`wxec` is distributed under the mit license. contributions, issues, and feature proposals are welcome at [https://github.com/kaeinmotion/wxec](https://github.com/kaeinmotion/wxec).
