wxec

[![ci](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![license: mit](https://img.shields.io/badge/license-mit-yellow.svg)](https://opensource.org/licenses/MIT)
[![model context protocol](https://img.shields.io/badge/mcp-compatible-green.svg)](https://modelcontextprotocol.io)

wxec is a high-throughput, token-efficient model context protocol execution engine for ai coding agents. it eliminates shell subprocess noise and context-window bloat by routing agent operations through structured, simd-accelerated workspace primitives, session hash ledgers, and atomic lossless memory handles.

documentation

| document | description |
| :--- | :--- |
| [docs/guide.md](docs/guide.md) | architecture, benchmarks, 1-click client configurations, and batching workflows |
| [docs/tools.md](docs/tools.md) | mcp tool specifications, parameter schemas, and operational boundaries |
| [docs/security.md](docs/security.md) | threat model, path jailing, credential redaction, and permission tiers |
| [docs/wire-format-v0.2.md](docs/wire-format-v0.2.md) | wire protocol serialization, handle storage, and token estimators |

quick start

build from source:
```sh
git clone https://github.com/kaeinmotion/wxec.git
cd wxec && cargo build --release
cp target/release/wxec ~/.cargo/bin/
```

install via smithery:
```sh
npx -y @smithery/cli install kaeinmotion/wxec --client cursor
```

run via docker:
```sh
docker run -i --rm -v "${PWD}:/workspace" ghcr.io/kaeinmotion/wxec
```

license

mit.
