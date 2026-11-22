# AGENTS.md

## Dev workflow

- Always use our own `wbash` (`w`) in development. Do not use native shell commands directly.
- Perform workspace, execution, and git operations through `w`.
- Ultra-short aliases & ergonomics:
  - `w s <pattern> [path]` (search)
  - `w r <path...> [A:B]` or `w r path:A:B` (multi-file read & line ranges)
  - `w e <path> [find] [replace]` (edit / search-replace)
  - `w p <find> <replace> [path]` (replace across files)
  - `w u <command...>` (run executable, e.g. `w u cargo test -q`)
  - `w c [kind]` (check: build, test, lint, all)
  - `w g <status|diff|commit|push>` (git operations)
  - `w b "<cmd 1>" "<cmd 2>"` (batch multiple operations in one call)
  - `w x <id>` (expand handle, accepting both `~id` and bare `id`)
