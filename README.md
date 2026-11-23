# wxec

[![ci](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![license: mit](https://img.shields.io/badge/license-mit-yellow.svg)](https://opensource.org/licenses/MIT)
[![model context protocol](https://img.shields.io/badge/mcp-compatible-green.svg)](https://modelcontextprotocol.io)

ultra-low-token model context protocol (mcp) execution engine for AI coding agents. routes agent operations through structured primitives, session hash ledgers, and atomic memory handles (`~id`) to eliminate prompt bloat and shell noise.

---

## documentation

all architectural specifications, benchmarks, tool catalogs, and client setups live in [`docs/`](docs/):

| document | contents |
| :--- | :--- |
| **[docs/guide.md](docs/guide.md)** | architecture, benchmarks (-83.7% tokens), 1-click client setups, and batching workflows |
| **[docs/tools.md](docs/tools.md)** | complete mcp tool specifications, schemas, and operational boundaries |
| **[docs/security.md](docs/security.md)** | threat model, path jailing, credential redaction, and permission tiers |
| **[docs/wire-format-v0.2.md](docs/wire-format-v0.2.md)** | wire protocol serialization, handle store (`~id`), and token estimators |

---

## quick start

**install binary (macos & linux):**
```sh
curl -fsSL https://raw.githubusercontent.com/kaeinmotion/wxec/main/scripts/install.sh | sh
```

**from source:**
```sh
cargo install --git https://github.com/kaeinmotion/wxec.git wxec-cli
```

**via npm wrapper:**
```sh
npx -y @kae/wxec
```

**via smithery:**
```sh
npx -y @smithery/cli install kaeinmotion/wxec --client claude
```

---

## license

mit.
