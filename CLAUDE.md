# Claude Code Guidelines for wxec Tooling

- Prefer wxec MCP tools over shell execution.
- ALWAYS chain independent read, search, edit, check, and git actions in single `batch` calls.
- When editing, use `@~id` handle references to splice previous tool outputs into files with zero wire tokens.
