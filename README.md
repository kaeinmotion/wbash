# wxec

[![ci](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![license: mit](https://img.shields.io/badge/license-mit-yellow.svg)](https://opensource.org/licenses/MIT)
[![model context protocol](https://img.shields.io/badge/mcp-compatible-green.svg)](https://modelcontextprotocol.io)

wxec is a high-throughput, ultra-low-token Model Context Protocol (MCP) execution engine for AI coding agents. it eliminates shell subprocess noise, terminal escape clutter, and transcript compounding by routing agent operations through structured, SIMD-accelerated workspace primitives, session hash ledgers, and atomic lossless memory handles (`~id`).

---

## empirical benchmarks

measured across real-world developer workflows using the standard `cl100k_base` tokenizer:

| task | raw shell | rtk (0.37.2) | wxec mcp | token savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| `git status` | 86 tok | 22 tok | **16 tok** | -81.4% |
| `git diff` | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| `read file` (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| `search / grep` | 567 tok | 459 tok | **11 tok** | -98.1% |
| `test execution` | 356 tok | 99 tok | **10 tok** | -97.2% |
| **total benchmark tokens** | **6,257 tok** | **5,381 tok** | **1,018 tok** | **-83.7%** |

---

## quick start

### 1. install binary

**one-line installer (macos & linux):**
```sh
curl -fsSL https://raw.githubusercontent.com/kaeinmotion/wxec/main/scripts/install.sh | sh
```

**from source via cargo:**
```sh
cargo install --git https://github.com/kaeinmotion/wxec.git wxec-cli
```

**via npm wrapper:**
```sh
npm install -g @kae/wxec
```

**via smithery:**
```sh
npx -y @smithery/cli install kaeinmotion/wxec --client claude
```

**run via docker:**
```sh
docker run -i --rm -v "${PWD}:/workspace" ghcr.io/kaeinmotion/wxec
```

---

## client configuration

add `wxec` to your client configuration file (`claude_desktop_config.json`, `.cursor/mcp.json`, `cline_mcp_settings.json`, or Antigravity):

```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": [
        "--root", "/path/to/workspace",
        "--tier", "git_write"
      ]
    }
  }
}
```

*or without pre-installing using npx:*
```json
{
  "mcpServers": {
    "wxec": {
      "command": "npx",
      "args": ["-y", "@kae/wxec", "--root", "/path/to/workspace"]
    }
  }
}
```

---

## core capabilities

- **1-line batch rollup (`batch`)**: bundles multiple tool calls into a single round-trip. state-modifying actions collapse into an exact 1-line net digest (e.g. `batch rollup: 2 ok (+2 -2, 83 passed)`), cutting turn latency and prompt bloat.
- **consecutive edit hash forwarding**: sequential same-file edits in a batch track BLAKE3 hash continuity automatically, eliminating false-positive hash mismatches.
- **session hash ledger (`read`)**: unchanged files return an instant 27-token acknowledgment (`unchanged <path>`) instead of re-emitting full text.
- **diagnostic noise suppression (`check`)**: compiler warnings are automatically omitted when build errors exist, keeping model attention 100% focused on actionable failures.
- **lossless handle store (`expand`)**: oversized logs, diffs, and directory trees are offloaded to atomic cache handles (`~id`) and expanded on demand.
- **zero-downtime hot reload**: running MCP server processes detect binary updates on disk and re-execute in place via Unix `execv` without dropping stdio pipes.
- **scoped & heuristic symbol search**: resolves qualified method lookups (`Type::method`) and falls back to regex brace-balanced symbol extraction when AST outlines miss.
- **bounded git operations (`git`)**: caps massive status and diff listings to prevent prompt flooding.

---

## mcp tools

| tool | tier | description | key parameters |
| :--- | :---: | :--- | :--- |
| `batch` | read_only | bundles multiple tool calls in 1 turn with 1-line rollup | `calls: [{"tool": "...", "args": {...}}]`, `stop_on_error` |
| `read` | read_only | token-efficient file, line-range, or symbol reader | `path`, `lines` (`"1:50"`), `symbol`, `force` |
| `edit` | write_workspace | surgical anchor & AST editor with near-line matching | `path`, `find`, `replace`, `lines`, `occurrence` |
| `replace` | write_workspace | multi-file pattern replacement with 1-line summary | `find`, `replace`, `path`, `glob`, `word`, `regex` |
| `search` | read_only | SIMD-accelerated regex/text search grouped by file | `pattern`, `path`, `glob`, `mode` (`text\|regex\|symbol`) |
| `check` | execute | project build, test, and lint runner with deduplication | `kind` (`build\|test\|lint\|all`), `path`, `args` |
| `git` | git_write | compact porcelain git operations | `op` (`status\|diff\|commit\|add\|log\|branch\|push`), `message` |
| `tree` | read_only | compact directory hierarchy with small-leaf inlining | `path`, `depth`, `dirs_only` |
| `expand` | read_only | expands lossless elided memory handles | `id` (`"~id"` or bare id), `lines` |
| `run` | execute | direct executable execution with bounded output | `argv: ["cargo", "test", ...]`, `timeout_ms` |
| `sh` | execute | shell command execution in sandbox | `command: "..."` |

---

## documentation

| document | description |
| :--- | :--- |
| [docs/guide.md](docs/guide.md) | architecture, benchmarks, 1-click client configurations, and batching workflows |
| [docs/tools.md](docs/tools.md) | mcp tool specifications, parameter schemas, and operational boundaries |
| [docs/security.md](docs/security.md) | threat model, path jailing, credential redaction, and permission tiers |
| [docs/wire-format-v0.2.md](docs/wire-format-v0.2.md) | wire protocol serialization, handle storage, and token estimators |

---

## license

mit.
