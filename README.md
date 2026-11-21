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
w tree [path] [--depth n]
w search <pattern> [path]
w read <path> [--lines a:b] [--outline]
w edit <path> --find <old> --replace <new>
w replace <old> <new> [path]
w --tier execute run -- <command...>
w --tier execute check [--kind all|build|lint|types|test]
w git <status|diff|commit|push>
w expand ~<id>
w batch '[{"tool":"read","args":{"path":"src/main.rs"}}]'
```

note: you need to override the native tool calling to fully utilize wbash. kindly tell your agent to do it for u.

## license

mit
