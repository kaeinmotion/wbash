# output parsers & diagnostic normalization

reference guide for built-in diagnostic extractors, stream parsers, and custom toml parser definitions in wxec.

---

### **table of contents**

- [chapter 1: stream parsing architecture](#chapter-1-stream-parsing-architecture)
- [chapter 2: native built-in parsers](#chapter-2-native-built-in-parsers)
- [chapter 3: fallback compaction heuristics](#chapter-3-fallback-compaction-heuristics)
- [chapter 4: custom toml parser definitions](#chapter-4-custom-toml-parser-definitions)

---

### **chapter 1: stream parsing architecture**

when `check` or `run` executes compilers, test runners, or linters, raw stdout/stderr can emit tens of thousands of lines. wxec intercepts output streams, filters boilerplate noise (such as progress indicators and passing test lines), and extracts structured `Diagnostic` and `TestResult` records.

critical failure lines, assertion diffs, and compiler error spans are prioritized in the response, while the unedited stream is preserved in a memory handle (`full: ~id`).

---

### **chapter 2: native built-in parsers**

wxec includes zero-dependency native parsers compiled into the binary:

| parser | matching commands | extracted signals |
| :--- | :--- | :--- |
| `cargo` | `cargo test`, `check`, `build`, `clippy` | json compiler diagnostics (`E####` error codes, spans), libtest event streams, panic messages, failing assertions |
| `pytest` | `pytest`, `py.test`, `python -m pytest` | `FAILED test_file.py::test_fn - message`, traceback frames, pass/fail summaries |
| `jest` / `vitest` | `jest`, `vitest`, `npm test` | test suite status, failed assertion diffs, file and line locations |
| `tsc` | `tsc`, `npx tsc` | typescript compiler errors: `file(line,col): error TS####: message` |
| `eslint` | `eslint`, `npx eslint` | file path, rule identifier, severity, line/column coordinates |
| `ruff` | `ruff check` | python linter diagnostics, error codes, suggested fixes |
| `go-test` | `go test` | package status, `--- FAIL: TestName` traces, assertion outputs |
| `go-vet` | `go vet` | static analysis warnings: `file.go:line:col: message` |

---

### **chapter 3: fallback compaction heuristics**

if a command does not match any specialized parser, wxec applies generic stream compaction:
- **ansi stripping**: removes all terminal color codes and cursor movement escape sequences.
- **whitespace normalization**: condenses blank lines and trailing spaces.
- **error line preservation**: scans for keywords (`error:`, `fatal:`, `failed:`, `panic:`, `exception:`) and prioritizes them in the output budget.
- **head + tail windowing**: emits initial setup lines, an elision marker `...+N lines (~id)`, and the final summary footer.
- **full handle generation**: stores complete unedited output in `full: ~id` for lossless inspection via `expand`.

---

### **chapter 4: custom toml parser definitions**

teams can register custom parser rules for proprietary test frameworks or internal build tools without recompiling wxec:

- **user rules**: `~/.config/wxec/parsers.d/*.toml`
- **project rules**: `<workspace-root>/.wxec/parsers.d/*.toml` (requires trusted root)

#### **example toml parser definition (`demo-test.toml`)**
```toml
schema_version = 1
name = "custom-test-runner"
priority = 100

[match]
argv0 = "mytest"
argv_contains = ["--run"]

[inject]
args = ["--format=jsonl"]

[parse]
format = "jsonl"

[map.test_result]
id = "$.test_name"
status = "$.result"
message = "$.error_details"

[map.status]
ok = "passed"
bad = "failed"
```

- **declarative safety**: toml parsers cannot execute arbitrary shell code; they only map json fields to normalized diagnostics.
