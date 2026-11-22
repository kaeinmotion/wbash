### **wxec**

[![ci](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![license: mit](https://img.shields.io/badge/license-mit-yellow.svg)](https://opensource.org/licenses/MIT)
[![glama quality score](https://glama.ai/mcp/servers/kaeinmotion/wxec/badges/score.svg)](https://glama.ai/mcp/servers/kaeinmotion/wxec)
[![model context protocol](https://img.shields.io/badge/mcp-compatible-green.svg)](https://modelcontextprotocol.io)

ultra token-efficient model context protocol (mcp) server for ai coding agents (claude, cursor, cline, windsurf, antigravity).

standard shell execution floods your context window with compiler traces, test logs, and whole files. wxec connects ai agents directly to your workspace, cuts context tokens by up to **84%**, eliminates ide permission popups, and executes multiple tools in a single sub-10ms turn with lossless memory handles.

### **why wxec mcp?**

- **84% fewer tokens**: slashes context usage from 6,257 to 1,018 tokens across everyday developer tasks.
- **zero permission popups**: connects natively via mcp, eliminating constant ide sandbox approval prompts.
- **2x+ faster agent turnaround**: cutting terminal junk eliminates 90+ seconds of model generation wait time.
- **100% self-contained**: pure rust with zero external system dependencies.
- **single-turn multi-action**: bundles up to 16 operations (read, edit, test, git) in one turn using `batch`.
- **lossless memory handles**: full outputs are preserved off-context and retrieved on demand with `expand`.

### **benchmark proof**

tested on real tasks with the `cl100k_base` tokenizer:

| task | raw shell | rtk (0.37.2) | wxec mcp | token savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| `git status` | 86 tok | 22 tok | **16 tok** | -81.4% |
| `git diff` | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| `read file` (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| `search / grep` | 567 tok | 459 tok | **11 tok** | -98.1% |
| `test execution` | 356 tok | 99 tok | **10 tok** | -97.2% |
| **total benchmark tokens** | **6,257 tok** | **5,381 tok** | **1,018 tok** | **-83.7%** |

### **quick setup**

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

for cursor, claude desktop, cline, and docker configs, see [complete client guide](docs/guide.md#client-configurations).

### **available mcp tools**

| tool | tier | purpose | key parameters |
| :--- | :---: | :--- | :--- |
| `read` | read_only | compact file & symbol inspector | `path`, `lines` (`"1:50"` or `"1-50"`), `force` |
| `edit` | write_workspace | surgical exact code editor | `path`, `find`/`target`, `replace`/`replacement` |
| `search` | read_only | simd-accelerated workspace search | `pattern`/`query`, `path`, `mode` (`text`/`regex`/`symbol`) |
| `replace` | write_workspace | global multi-file pattern replacement | `find`, `replace`, `path`, `glob`, `dry_run` |
| `tree` | read_only | structured directory hierarchy | `path`, `depth`, `sizes`, `dirs_only` |
| `check` | execute | project compiler and test checker | `kind` (`test`, `build`, `lint`, `type`, `all`) |
| `git` | git_write | complete git lifecycle | `op` (`status`, `add`, `diff`, `commit`, `push`, `log`) |
| `batch` | per-call | bundle up to 16 calls in 1 turn | `calls: [{"tool": "...", "args": {...}}]` |
| `expand` | read_only | retrieve full text from handle | `id` (e.g. `"~h123"`), `lines`, `grep` |
| `run` | execute | structured executable runner | `argv`/`cmd`, `cwd`, `timeout_ms` |
| `sh` | execute | shell execution fallback | `command`, `cwd`, `timeout_ms` |

### **documentation**

- [user guide & client setup](docs/guide.md): full client configurations, batching recipes, and `.wxec.toml` options.
- [tool specification](docs/tools.md): detailed semantics, parameters, and input schemas for every tool.
- [security model](docs/security.md): workspace jailing, permission tiers, and secret redaction.
- [wire format](docs/wire-format-v0.2.md): protocol specification and token estimator mechanics.

### **license**

mit
