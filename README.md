# wxec

token-efficient execution layer and model context protocol (mcp) server for ai coding agents.

## why wxec mcp?

raw shell tools dump thousands of tokens of compiler noise, test logs, and boilerplate into your context window. alternative cli proxies like rtk still dump full files and require external system binaries. `wxec` is built from the ground up in pure rust as an mcp-first execution engine:

- **84% token reduction vs raw shell**: reduces token consumption from 6,257 to 1,018 tokens across real-world workflows.
- **81% token reduction vs rtk**: avoids dumping entire files by using smart outline views and session deduplication.
- **zero permission popups**: runs natively as an mcp server inside cursor, antigravity, and claude desktop with no constant approval prompts.
- **2x+ faster agent turnaround**: eliminating terminal spam cuts llm generation wait times in half.
- **100% self-contained**: pure rust implementation with zero external dependencies (unlike tools that crash if `tree` is missing).
- **batching in a single turn**: execute up to 16 tool calls (read, edit, test, git) in one round-trip with the `batch` tool.
- **session hash ledger**: unmodified files return instant "unchanged" status tokens instead of re-reading entire contents.
- **lossless handle store (`~<id>`)**: full traces and files are preserved off-context and can be retrieved on demand with `expand`.

## benchmark (real cl100k_base tokens)

| task | raw shell | rtk (0.37.2) | wxec mcp | savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| git status | 86 tok | 22 tok | **16 tok** | -81.4% |
| git diff | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| file read (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| search / grep | 567 tok | 459 tok | **11 tok** | -98.1% |
| test execution | 356 tok | 99 tok | **10 tok** | -97.2% |
| total | 6,257 tok | 5,381 tok | **1,018 tok** | **-83.7%** |

## setup (mcp first)

add `wxec` to your mcp configuration (e.g. `~/.gemini/config/mcp_config.json`, claude desktop, cursor, or antigravity):

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

### installation

```sh
git clone https://github.com/kaeinmotion/wxec.git
cd wxec && cargo build --release
cp target/release/wxec ~/.cargo/bin/
```

## available mcp tools

| tool | purpose | key parameters |
| :--- | :--- | :--- |
| `read` | token-compact file inspector | `path`, `lines` (`"1:50"` or `"1-50"`), `force` |
| `edit` | surgical code editor with aliases | `path`, `find`/`target`, `replace`/`replacement` |
| `search` | simd-accelerated workspace search | `pattern`/`query`, `path`, `mode` (`text`/`regex`/`symbol`) |
| `replace` | global pattern replacement | `find`, `replace`, `path`, `glob` |
| `tree` | dependency-free directory tree | `path`, `depth` |
| `check` | fast compiler and test checks | `kind` (`test`, `build`, `lint`, `all`) |
| `git` | complete git lifecycle | `op` (`status`, `add`, `diff`, `commit`, `push`, `log`) |
| `batch` | multi-tool bundling in 1 turn | `calls: [{"tool": "...", "args": {...}}]` |
| `expand` | retrieve full text from handle | `id` (e.g. `"~h123"`) |
| `run` | structured command executor | `argv`/`cmd`, `cwd` |
| `sh` | shell fallback with timeout safety | `command` |

## license

mit
