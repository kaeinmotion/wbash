# wxec

[![ci](https://github.com/kaeinmotion/wxec/actions/workflows/ci.yml/badge.svg)](https://github.com/kaeinmotion/wxec/actions)
[![release](https://img.shields.io/github/v/release/kaeinmotion/wxec?color=blue)](https://github.com/kaeinmotion/wxec/releases/latest)
[![license: mit](https://img.shields.io/badge/license-mit-yellow.svg)](https://opensource.org/licenses/MIT)
[![model context protocol](https://img.shields.io/badge/mcp-compatible-green.svg)](https://modelcontextprotocol.io)

wxec is an ultra token-efficient execution engine and model context protocol (mcp) server built for ai coding agents. it bridges autonomous tools (such as claude, cursor, opencode, cline, and antigravity) directly to your project workspace. by replacing noisy terminal outputs with compressed, structured data, wxec lowers ai api costs by up to 84% while accelerating multi-step development workflows.

---

### **table of contents**

- [chapter 1: executive overview](#chapter-1-executive-overview)
- [chapter 2: core mechanics & architecture](#chapter-2-core-mechanics--architecture)
- [chapter 3: documentation library](#chapter-3-documentation-library)
- [chapter 4: performance & cost savings](#chapter-4-performance--cost-savings)
- [chapter 5: quick start & installation](#chapter-5-quick-start--installation)
- [chapter 6: client integrations](#chapter-6-client-integrations)
- [chapter 7: security & governance](#chapter-7-security--governance)
- [chapter 8: license](#chapter-8-license)

---

### **chapter 1: executive overview**

#### **the business case: cost and reliability**
every time an ai agent inspects files, runs tests, or checks git history, it consumes tokens that you pay for per thousand units. raw terminal commands flood the context window with repetitive boilerplate, resulting in:
- **exponential cost scaling**: large codebases burn through context budgets within minutes.
- **context degradation**: models lose focus ("lost in the middle") when drowned in compiler logs and large file dumps.
- **slow turnaround**: agents spend multiple back-and-forth round trips asking for information they already saw.

wxec solves this at the protocol level. by substituting raw shell text with compact, structured data, teams save an average of 83.7% in token expenses while completing complex coding tasks 3x faster.

#### **the engineering case: structured precision**
traditional ai agents rely on fragile shell commands (`grep`, `sed`, `awk`) that often break due to string escaping and operating system quirks. wxec replaces shell work with dedicated mcp tools:
- **surgical file edits**: line-range and anchor-based matching that prevents syntax corruption.
- **zero shell injection**: commands run through guarded, typed interfaces.
- **1-turn workflow execution**: bundle up to 16 file operations into a single api call.

---

### **chapter 2: core mechanics & architecture**

wxec achieves breakthrough efficiency through four core innovations:

1. **session hash ledger**: when an agent inspects a file, wxec records its cryptographic hash. if the file has not changed in later turns, wxec returns a tiny acknowledgment (27 tokens) instead of re-reading thousands of lines. token savings: **-99.2%**.
2. **lossless handle store (`~id`)**: massive compiler outputs, test suites, and directory trees are saved into local atomic cache handles (like `~h101`). the agent receives a concise summary and can expand specific sections on demand, keeping working memory lean.
3. **simd workspace cache**: searches use vectorized in-memory substring algorithms (`memmem`), scanning 50,000 files in under 11 milliseconds.
4. **1-turn batching engine**: through the `batch` tool, an agent can search, read, edit, and run tests all in one round-trip request, eliminating wasted network delays.

---

### **chapter 3: documentation library**

all specialized guides, architectural specifications, and tooling manuals are cataloged below:

| document | description |
| :--- | :--- |
| [docs/guide.md](docs/guide.md) | complete user guide, 1-click client setups (cursor, claude, opencode, cline, docker), benchmarks, and configuration |
| [docs/tools.md](docs/tools.md) | available mcp tools, parameter schemas, and real-world usage examples |
| [docs/security.md](docs/security.md) | enterprise security model, workspace jailing, secret redaction, and permission tiers |
| [docs/wire-format-v0.2.md](docs/wire-format-v0.2.md) | protocol specifications, handle store mechanics, and token estimation math |

---

### **chapter 4: performance & cost savings**

the following benchmark compares standard developer tasks measured against the `cl100k_base` tokenizer:

| task | raw shell | rtk (cli proxy) | wxec (mcp engine) | savings vs raw |
| :--- | :---: | :---: | :---: | :---: |
| `git status` | 86 tok | 22 tok | **16 tok** | -81.4% |
| `git diff` | 1,661 tok | 1,319 tok | **874 tok** | -47.4% |
| `read file` (480 lines) | 3,422 tok | 3,422 tok | **27 tok** | -99.2% |
| `search / grep` | 567 tok | 459 tok | **11 tok** | -98.1% |
| `test execution` | 356 tok | 99 tok | **10 tok** | -97.2% |
| **total benchmark cost** | **6,257 tok** | **5,381 tok** | **1,018 tok** | **-83.7%** |

---

### **chapter 5: quick start & installation**

#### **option a: build from source**
requires a standard rust toolchain:
```sh
git clone https://github.com/kaeinmotion/wxec.git
cd wxec && cargo build --release
cp target/release/wxec ~/.cargo/bin/
```

#### **option b: one-click installer (smithery)**
install automatically into your chosen ai assistant:
```sh
npx -y @smithery/cli install kaeinmotion/wxec --client cursor
```

#### **option c: containerized run (docker)**
run without local dependencies:
```sh
docker run -i --rm -v "${PWD}:/workspace" ghcr.io/kaeinmotion/wxec
```

---

### **chapter 6: client integrations**

#### **cursor** (`.cursor/mcp.json`)
```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": ["--tier", "git_write"]
    }
  }
}
```

#### **opencode** (`opencode.json`)
```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "wxec": {
      "type": "local",
      "command": ["wxec", "--tier", "git_write"],
      "enabled": true
    }
  }
}
```

#### **claude code cli**
```sh
claude mcp add wxec -- wxec --tier git_write
```

#### **cline / roo code** (`cline_mcp_settings.json`)
```json
{
  "mcpServers": {
    "wxec": {
      "command": "wxec",
      "args": ["--tier", "git_write"],
      "disabled": false,
      "autoApprove": ["read", "search", "tree", "expand"]
    }
  }
}
```

---

### **chapter 7: security & governance**

enterprise ai adoption demands strict boundaries. wxec provides defense-in-depth:
- **workspace jailing**: agents cannot escape the configured root project directory.
- **automated secret redaction**: real-time regexset scanning strips api keys, private tokens, and environment credentials before they reach the model.
- **permission tiers**: configure granular permissions (`read_only`, `write_workspace`, or `git_write`) to enforce safety standards across teams.

---

### **chapter 8: license**

mit. open source and free for personal, commercial, and enterprise usage.
