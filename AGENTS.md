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
- **`read`**: Read files or line ranges (`path`, `lines`, `context`, `symbol`, `paths`). Supports `#symbol` extraction, `:line` syntax, and auto-resolves unique workspace path suffixes.
- **`edit`**: Surgical file editor (`path`, `find`/`lines`, `replace`, `occurrence`, `all`). Supports direct handle references (`replace: "~id"`), `#symbol` targeting, and auto-threads hashes across chained calls.
- **`replace`**: Multi-file replacements (`find`, `replace`, `path`, `glob`, `word`, `regex`). Supports direct handle interpolation (`replace: "~id"`).
- **`search`**: Search patterns across workspace (`pattern`, `path`, `mode`, `context`, `files_only`). Use `files_only: true` for ultra-low-token sweeps.
- **`check`**: Project diagnostics and test runner (`kind: "test" | "build" | "lint" | "all"`, `filter: "<test>"`). Compact Net Digest rollup output.
- **`git`**: Git lifecycle (`op: "status" | "diff" | "commit" | "push" | "log"`, `all`, `tags`, `ref`, `message`).
- **`tree`**: Directory hierarchy (`path`, `depth`, `dirs_only`, `glob`, `sizes`).
- **`batch`**: Single-turn execution of up to 16 heterogeneous calls (`calls`, `compact: true`).
- **`expand`**: Retrieve compacted handle contents (`ref: "~id"` or `refs: ["~id1", "~id2"]`, `lines`, `head`).
- **`run`**: Run project executables with structured argv (`argv`, `cwd`, `timeout_ms`).
- **`sh`**: Bounded shell execution fallback when strictly necessary.
