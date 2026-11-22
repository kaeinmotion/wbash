### **wxec user guide**

comprehensive reference for configuring, running, and orchestrating wxec mcp across ai coding environments.

### **table of contents**

- [architecture & how it works](#architecture--how-it-works)
- [benchmarks](#benchmarks)
- [available mcp tools](#available-mcp-tools)
- [client configurations](#client-configurations)
- [configuration file (.wxec.toml)](#configuration-file-wxectoml)
- [1-turn batching workflows](#1-turn-batching-workflows)
- [security & permission tiers](#security--permission-tiers)

### **architecture & how it works**

- **session hash ledger**: tracks file reads and searches across agent turns; unchanged files return instant zero-cost hash acknowledgments instead of refetching the whole file.
- **lossless handle store (`~<id>`)**: offloads large diffs, test logs, and trees to atomic cache blobs, keeping context clean while allowing instant expansion via `expand`.
- **simd workspace cache**: in-memory directory tree with vectorized substring pre-matching (`memmem`) for 10x-50x faster searches.
- **calibrated budget fitting**: prioritizes actionable diagnostics, test failures, and line numbers while compressing low-priority noise into handles.

### **benchmarks**

tested on real developer tasks with the `cl100k_base` tokenizer:

| task | raw shell | rtk (0.37.2) | wxec mcp | token savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| `git status` | 86 tok | 22 tok | **16 tok** | -81.4% |
| `git diff` | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| `read file` (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| `search / grep` | 567 tok | 459 tok | **11 tok** | -98.1% |
| `test execution` | 356 tok | 99 tok | **10 tok** | -97.2% |
| **total benchmark tokens** | **6,257 tok** | **5,381 tok** | **1,018 tok** | **-83.7%** |

### **available mcp tools**

| tool | tier | purpose | key parameters |
| :--- | :---: | :--- | :--- |
| `read` | read_only | compact file & symbol inspector | `path`, `lines` (`"1:50"` or `"1-50"`), `force` |
| `edit` | write_workspace | surgical exact code editor | `path`, `find`/`target`, `replace`/`replacement` |
| `search` | read_only | simd-accelerated workspace search | `pattern`/`query`, `path`, `mode` (`text`/`regex`/`symbol`) |
| `replace` | write_workspace | global multi-file pattern replacement | `find`, `replace`, `path`, `glob`, `dry_run` |
| `tree` | read_only | structured directory hierarchy | `path`, `depth`, `sizes`, `dirs_only` |
| `check` | execute | project compiler and test checker | `kind` (`test`, `build`, `lint`, `type`, `all`) |
| `git` | git_write | complete git lifecycle | `op` (`status`, `add`, `diff`, `commit`, `push`, `log`) |
| `batch` | per-call | bundle up to 16 calls in 1 turn | `calls: [{"tool": "...", "args": {...}}]` |
| `expand` | read_only | retrieve full text from handle | `id` (e.g. `"~h123"`), `lines`, `grep` |
| `run` | execute | structured executable runner | `argv`/`cmd`, `cwd`, `timeout_ms` |
| `sh` | execute | shell execution fallback | `command`, `cwd`, `timeout_ms` |

### **client configurations**

#### **cursor** (`.cursor/mcp.json`)
```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": ["--tier", "git_write"]
    }
  }
}
```

#### **claude desktop** (`claude_desktop_config.json`)
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

#### **claude code cli**
```sh
claude mcp add wxec -- wxec --tier git_write
```

#### **cline / roo code** (`cline_mcp_settings.json`)
```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": ["--tier", "git_write"],
      "disabled": false,
      "autoApprove": ["read", "search", "tree", "expand"]
    }
  }
}
```

#### **docker (no local rust toolchain)**
```json
{
  "mcpServers": {
    "wxec": {
      "command": "docker",
      "args": ["run", "-i", "--rm", "-v", "${workspaceFolder}:/workspace", "ghcr.io/kaeinmotion/wxec"]
    }
  }
}
```

### **configuration file (.wxec.toml)**

optional `.wxec.toml` or `wxec.toml` placed in your workspace root:

```toml
[session]
default_tier = "git_write"
ledger_ttl_secs = 86400

[handles]
ttl_secs = 86400
max_bytes = 104857600

[exec]
allow_shell = true
timeout_ms = 120000

[output]
verbosity = "minimal"
```

### **1-turn batching workflows**

bundle multi-step workflows into a single round-trip turn to eliminate back-and-forth prompt latency:

```json
{
  "calls": [
    {
      "tool": "edit",
      "args": {
        "path": "crates/core/src/lib.rs",
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

when `stop_on_error: true` is passed, execution halts immediately after the first failing step and reports remaining calls as skipped.

### **security & permission tiers**

- **workspace jailing**: all paths are strictly confined inside workspace root; traversal escapes (`../`) are rejected.
- **secret redactor**: credentials, api keys, tokens, and private keys are scrubbed before reaching wire output.
- **permission tiers**:

| tier | level | authorized tools |
| :--- | :---: | :--- |
| `read_only` | 0 | `tree`, `read`, `search`, `expand`, `git` (status/diff/log) |
| `write_workspace` | 1 | `edit`, `replace` |
| `execute` | 2 | `run`, `check`, `sh` |
| `git_write` | 3 | `git add`, `git commit`, `git push` |
