wxec

[![ci](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![license: mit](https://img.shields.io/badge/license-mit-yellow.svg)](https://opensource.org/licenses/MIT)
[![model context protocol](https://img.shields.io/badge/mcp-compatible-green.svg)](https://modelcontextprotocol.io)

wxec is an ultra token-efficient model context protocol (mcp) server and execution engine for ai coding agents (claude, cursor, cline, windsurf, antigravity). it connects agents directly to your workspace, cuts context tokens by up to 84%, and executes multi-step workflows in a single turn with lossless memory handles.

documentation

| Document | Description |
| :--- | :--- |
| [docs/guide.md](docs/guide.md) | User guide, 1-click client configurations (Cursor, Claude, Cline, Docker), benchmarks, and configuration |
| [docs/tools.md](docs/tools.md) | Available MCP tools, parameter schemas, and usage examples |
| [docs/security.md](docs/security.md) | Security model, workspace jailing, and permission tiers |
| [docs/wire-format-v0.2.md](docs/wire-format-v0.2.md) | Wire protocol, handle store, and token estimators |

quick start

build from source:
```sh
git clone https://github.com/kaeinmotion/wxec.git
cd wxec && cargo build --release
cp target/release/wxec ~/.cargo/bin/
```

or install via smithery:
```sh
npx -y @smithery/cli install kaeinmotion/wxec --client cursor
```

license

mit.
