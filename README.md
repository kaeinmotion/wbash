# w-bash

token-efficient cli and mcp engine for ai coding agents.

## why

raw shell and tool outputs waste context windows. w-bash filters passing noise, formats structured results, deduplicates reads, and stores full logs in handles (`~id`) to minimize token consumption.

## install

```sh
git clone https://github.com/kaeinmotion/wbash.git
cd wbash && cargo build --release
```

- `target/release/w`: cli binary
- `target/release/w-bash`: stdio mcp server

## commands

```sh
w tree [path] [--depth n]            # alias: w t
w search <pattern> [path]            # alias: w s
w read <path...> [a:b] [path:a:b]    # alias: w r (supports multi-file & line ranges)
w edit <path> [find] [replace]       # alias: w e
w replace <old> <new> [path]         # alias: w p
w run <command...>                   # alias: w u (e.g. w u cargo test -q)
w check [--kind all|build|lint|test] # alias: w c
w git <status|diff|commit|push>      # alias: w g
w expand <id>                        # alias: w x (accepts 3ho or ~3ho)
w batch "<cmd 1>" "<cmd 2>"          # alias: w b (batch multiple tool calls)
```

note: you need to override the native tool calling to fully utilize wbash. kindly tell your agent to do it for u.

## license

mit
