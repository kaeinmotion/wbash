# wxec

Ultra-compact, token-efficient Model Context Protocol (MCP) server for AI coding agents.

## Why wxec?

Raw tool outputs waste thousands of context window tokens. `wxec` is built from the ground up for AI coding agents:
- **60% lower output token generation**: Strips compiler noise, collapses repeated test outputs, and formats results with maximum density.
- **Up to 10x smaller file reads**: Deduplicates unchanged file reads and returns compact slices with invariant handles (`~id`).
- **Batching in a single turn**: Executes multiple operations in one MCP round-trip (`batch` tool).

## Setup (MCP First)

Add `wxec` to your agent's MCP configuration (e.g. `~/.gemini/config/mcp_config.json`, Claude Desktop, Cursor, or Antigravity):

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

### Installation

```sh
git clone https://github.com/KlyneChrysler/wxec.git
cd wxec && cargo build --release
cp target/release/wxec ~/.cargo/bin/
```

## Available MCP Tools

| Tool | Purpose | Key Parameters |
| :--- | :--- | :--- |
| **`read`** | Token-compact file inspector | `path`, `lines` (e.g. `"1:50"`), `force` |
| **`edit`** | Surgical code editor | `path`, `find`/`lines`, `replace` |
| **`search`** | High-performance code search | `pattern`, `path`, `mode` (`text`/`regex`/`symbol`) |
| **`replace`** | Global pattern replacement | `find`, `replace`, `path`, `glob` |
| **`tree`** | Directory structure overview | `path`, `depth`, `dirs_only` |
| **`check`** | Zero-friction project checks | `kind` (`test`, `build`, `lint`, `all`) |
| **`git`** | Git status, diff, log, and commit | `op` (`status`, `diff`, `commit`, `log`) |
| **`batch`** | Multi-tool bundling in 1 turn | `calls: [{"tool": "...", "args": {...}}]` |
| **`expand`** | Unpack elided handle text | `id` (e.g. `"~3ho"`) |
| **`run`** | Executable runner | `argv: ["cargo", "test", ...]` |
| **`sh`** | Safe shell execution | `command` |

## License

MIT
