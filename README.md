### **wxec**

[![CI](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![Release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Glama Quality Score](https://glama.ai/mcp/servers/kaeinmotion/wxec/badges/score.svg)](https://glama.ai/mcp/servers/kaeinmotion/wxec)
[![Model Context Protocol](https://img.shields.io/badge/MCP-Compatible-green.svg)](https://modelcontextprotocol.io)

**Ultra token-efficient Model Context Protocol (MCP) server for AI coding agents (Claude, Cursor, Cline, Windsurf, Antigravity).**

`wxec` is an execution engine built in pure Rust that connects AI coding agents directly to your workspace. Standard shell commands flood your context window with compiler traces, test logs, and whole files. `wxec` cuts output tokens by up to **84%**, eliminates IDE permission dialogues, and enables sub-10ms single-turn multi-tool execution with lossless memory handles.

### **table of contents**

- [chapter 1: core benefits](#chapter-1-core-benefits)
- [chapter 2: how it works](#chapter-2-how-it-works)
- [chapter 3: benchmark proof](#chapter-3-benchmark-proof)
- [chapter 4: available mcp tools](#chapter-4-available-mcp-tools)
- [chapter 5: 1-turn batching](#chapter-5-1-turn-batching)
- [chapter 6: security & permission tiers](#chapter-6-security--permission-tiers)
- [chapter 7: quick setup](#chapter-7-quick-setup)
- [chapter 8: configuration](#chapter-8-configuration)

### **chapter 1: core benefits**

- **84% fewer tokens**: slashes context usage from 6,257 to 1,018 tokens across real-world developer workflows.
- **zero permission popups**: connects natively via mcp, eliminating constant ide sandbox approval prompts.
- **2x+ faster agent turnaround**: cutting terminal junk eliminates 90+ seconds of model generation wait time.
- **100% self-contained**: pure rust with zero external system dependencies (never crashes from missing host tools).
- **single-turn multi-action**: bundles up to 16 operations (read, edit, test, git) in one turn using `batch`.
- **lossless memory handles**: full outputs are preserved off-context and retrieved on demand with `expand`.

### **chapter 2: how it works**

- **session hash ledger**: deduplicates file reads and searches; unchanged files return instant zero-cost hash acknowledgments.
- **lossless handle store (`~<id>`)**: offloads large diffs and logs to atomic cache blobs, keeping context clean while allowing instant expansion.
- **simd workspace cache**: in-memory directory tree with vectorized substring pre-matching (`memmem`) for 10x-50x faster searches.
- **calibrated budget fitting**: prioritizes actionable diagnostics and line numbers while compressing low-priority noise into handles.

### **chapter 3: benchmark proof**

tested on real tasks with the `cl100k_base` tokenizer:

| task | raw shell | rtk (0.37.2) | wxec mcp | token savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| `git status` | 86 tok | 22 tok | **16 tok** | -81.4% |
| `git diff` | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| `read file` (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| `search / grep` | 567 tok | 459 tok | **11 tok** | -98.1% |
| `test execution` | 356 tok | 99 tok | **10 tok** | -97.2% |
| **total benchmark tokens** | **6,257 tok** | **5,381 tok** | **1,018 tok** | **-83.7%** |

### **chapter 4: available mcp tools**

| tool | tier | purpose | key parameters |
| :--- | :---: | :--- | :--- |
| `read` | read_only | compact file & symbol inspector | `path`, `lines` (`"1:50"` or `"1-50"`), `force` |
| `edit` | write_workspace | atomic surgical code editor | `path`, `find`/`target`, `replace`/`replacement` |
| `search` | read_only | simd-accelerated workspace search | `pattern`/`query`, `path`, `mode` (`text`/`regex`/`symbol`) |
| `replace` | write_workspace | global multi-file pattern replacement | `find`, `replace`, `path`, `glob`, `dry_run` |
| `tree` | read_only | structured directory hierarchy | `path`, `depth`, `sizes` |
| `check` | execute | project compiler and test checker | `kind` (`test`, `build`, `lint`, `type`, `all`) |
| `git` | git_write | complete git lifecycle | `op` (`status`, `add`, `diff`, `commit`, `push`, `log`) |
| `batch` | per-call | bundle up to 16 calls in 1 turn | `calls: [{"tool": "...", "args": {...}}]` |
| `expand` | read_only | retrieve full text from handle | `id` (e.g. `"~h123"`), `lines`, `grep` |
| `run` | execute | structured executable runner | `argv`/`cmd`, `cwd`, `timeout_ms` |
| `sh` | execute | shell execution fallback | `command`, `cwd`, `timeout_ms` |

### **chapter 5: 1-turn batching**

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

### **chapter 6: security & permission tiers**

- **workspace jailing**: all paths are strictly confined inside workspace root; traversal escapes (`../`) are blocked.
- **secret redactor**: credentials, aws keys, tokens, and private keys are scrubbed before reaching wire output.
- **permission tiers**:

| tier | level | authorized tools |
| :--- | :---: | :--- |
| `read_only` | 0 | `tree`, `read`, `search`, `expand`, `git` (status/diff/log) |
| `write_workspace` | 1 | `edit`, `replace` |
| `execute` | 2 | `run`, `check`, `sh` |
| `git_write` | 3 | `git add`, `git commit`, `git push` |

### **chapter 7: quick setup**

#### **installation**

Build from source:
```sh
git clone https://github.com/kaeinmotion/wxec.git
cd wxec && cargo build --release
cp target/release/wxec ~/.cargo/bin/
```

Or install via Smithery:
```sh
npx -y @smithery/cli install kaeinmotion/wxec --client cursor
```

#### **1-click client configurations**

**Cursor** (`.cursor/mcp.json`):
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

**Claude Desktop** (`claude_desktop_config.json`):
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

**Claude Code CLI**:
```sh
claude mcp add wxec -- wxec --tier git_write
```

**Cline / Roo Code** (`cline_mcp_settings.json`):
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

**Docker (No Local Rust Toolchain)**:
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

### **chapter 8: configuration**

optional `.wxec.toml` or `wxec.toml` in your workspace root:

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
