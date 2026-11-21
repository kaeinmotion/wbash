# w-bash

cli and mcp tools for ai coding agents.

runs file, search, read, edit, command, test, and git tasks with less output. keeps errors, test failures, file locations, exit codes, and changed lines. use `expand ~id` for full output.

## build

needs rust and git.

```sh
git clone https://github.com/klynechrysler/wbash.git
cd wbash && cargo build --release
```

- `target/release/w`: cli
- `target/release/w-bash`: mcp server

## usage

```sh
w [--root dir] [--session id] [--format lean|json] [--budget n] [--tier read_only|write_workspace|execute|git_write] <tool> [args]

w tree src --depth 2
w search "pattern" src
w read src/lib.rs --lines 1:60
w edit src/lib.rs --find 'old' --replace 'new'
w replace old new src
w --tier execute run -- cargo test -q
w --tier execute check --kind all
w git status
w --tier git_write git commit -m "message"
w expand ~id
```
```
note: you need to override the native tool calling to fully utilize wbash. kindly tell your agent to do it for u
```

use `--` before child args for `run`/`sh` when ambiguous. `run` CLI control flags: `--cwd`, `--timeout-ms`, `--stdin`, `--env`.

## mcp

```sh
w-bash --root /path/to/project
```

stdio server for agent clients.

## config

- user: `~/.config/w-bash/config.toml`
- project: `.w-bash/config.toml` (trusted only)
- `w config trust`
- `w config show`

## license

mit
