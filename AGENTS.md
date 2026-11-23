# AGENTS.md

## Core Developer Mission

Strictly and always prioritize:
1. **MCP Only**: Always and exclusively use `wxec` MCP tools for all workspace, code, search, read, edit, check, and git operations. Do not use native raw tools or direct CLI execution.
2. **Super Low Token Usage**: Our primary mission is ultra-low token consumption. Every byte and token matters. Eliminate prompt noise, redundant reads, terminal escape sequences, and verbose outputs. Use handles (`~id`) and targeted ranges.
3. **Legacy System Support via MCP**: Enable agents operating in modern or legacy terminal environments (bash, zsh, tmux, scripts, CI/CD) to interface safely and cleanly through the MCP protocol without spawning token-wasting CLI subshells.
4. **Way Lesser Tool Calls**: Batch and chain operations using the `batch` tool (`call_mcp_tool(ServerName="wxec", ToolName="batch", ...)`). Solve in 1-2 turns, not dozens.
5. **Way Lesser Output**: Zero fluff, no redundant hashes, minimal headers, maximum information density.

## Dev Workflow (MCP Only)

Always use `wxec` MCP tools via `call_mcp_tool(ServerName="wxec", ToolName=...)`:
- **`read`**: Read files or line ranges (`path`, `lines`). Deduplicates repeated reads and provides token-efficient compact summaries.
- **`edit`**: Modify files (`path`, `find`/`lines`, `replace`). Supports line-range replacements and whitespace-tolerant fuzzy anchors.
- **`replace`**: Replace across files (`find`, `replace`, `path`, `glob`).
- **`search`**: Search patterns across files (`pattern`, `path`, `mode`, `context`).
- **`check`**: Run project checks (`kind: "test" | "build" | "lint" | "all"`). Bounded execution without raw shell prompts.
- **`git`**: Git operations (`op: "status" | "diff" | "commit" | "log"`).
- **`tree`**: List workspace directory/file hierarchy (`path`, `depth`, `dirs_only`).
- **`batch`**: Bundle multiple tool calls into a single round-trip request (`calls: [{"tool": "...", "args": {...}}]`).
- **`expand`**: Expand compacted handles (`id: "~h123"`).
- **`run`**: Run project executables (`argv: ["cargo", "test", ...]`).
- **`sh`**: Shell command execution when needed.
