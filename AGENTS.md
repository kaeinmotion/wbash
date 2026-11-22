# AGENTS.md

## Core Developer Mission

Strictly and always prioritize:
1. **Super Short Commands**: Save prompt/input tokens. Never require boilerplate flags or paths.
2. **Way Lesser Tool Calls**: Batch and chain operations (`w b "..."`, multi-file reads `w r f1 f2`). Solve in 1-2 turns, not dozens.
3. **Way Lesser Output**: Zero fluff, no redundant hashes, minimal headers, maximum information density.

## Dev workflow

- Always use our own `wbash` (`w`) in development. Do not use native shell commands directly.
- Perform workspace, execution, and git operations through `w`.
- Ultra-short aliases & ergonomics:
  - `w s <pattern> [path]` (search)
  - `w r <path...> [A:B]` or `w r path:A:B` (multi-file read & line ranges)
  - `w e <path> [find/lines] [replace]` (edit: exact/whitespace-tolerant anchor or line range `f.rs 10:20 "code"`)
  - `w p <find> <replace> [path]` (replace across files)
  - `w u <command...>` (run executable, e.g. `w u cargo test -q`)
  - `w c [kind]` (check: build, test, lint, all)
  - `w g <status|diff|commit|push>` (git operations)
  - `w b "<cmd 1>" "<cmd 2>"` (batch multiple operations in one call; chain with `;` or `&&`)
  - `w x <id>` (expand handle, accepting both `~id` and bare `id`)
