# Output parsers

`run` and `check` map command output to normalized `Diagnostic` and
`TestResult` records. Parsing is selected by argv and applied to stdout; stderr
is scanned as a fallback. Parsing may use injected machine-readable flags/streams; user-visible output,
compact tails, and stored full handles are derived from user-relevant output with
injected machine records stripped when not required to preserve signals. Lossless
access remains available via handles/expand as applicable.

## Native parsers

| Parser | Matches | Extracts |
|---|---|---|
| `cargo` | `cargo test`/`check`/`build`/`clippy` | JSON `compiler-message` diagnostics (`E####` codes, spans); JSON libtest events; human `test ... ok/FAILED` and panic messages |
| `pytest` | `pytest`/`py.test` | `FAILED id - message`, pass/fail summary |
| `jest` | `jest` | `testResults[].assertionResults[]` JSON (name, status, first failure line, duration) |
| `vitest` | `vitest` | same JSON shape |
| `tsc` | `tsc` | `file(line,col): error TS####: message` |
| `eslint` | `eslint` | JSON formatter messages (file, line/col, rule id, severity) |
| `ruff` | `ruff check` | JSON diagnostics (filename, row/col, code, message) |
| `go-test` | `go test` | `--- PASS/FAIL: Test...` and package summaries |
| `go-vet` | `go vet` | `file.go:line:col: message` |

If no parser matches, `run` uses generic compaction (§7.5): ANSI stripping,
whitespace normalization, error-line preservation, head+tail windows, and a
`# parse_fallback` advisory. Exit code, duration, and must-keep errors are still
emitted.

## TOML parsers (v1)

User rules: `~/.config/wxec/rules.d/*.toml` or `~/.config/wxec/parsers.d/*.toml`.
Project rules: `<root>/.wxec/rules.d/*.toml` or `<root>/.wxec/parsers.d/*.toml` (trusted root only, disabled by `--no-project-config`).
Lower `priority` wins among matches.

```toml
schema_version = 1
name = "demo"
priority = 100

[match]
argv0 = "demo"
argv_contains = ["run"]

[inject]
args = ["--json"]

[parse]
format = "jsonl"            # json | jsonl

[map.test_result]
id = "$.name"
status = "$.state"
message = "$.msg"

[map.status]
ok = "pass"
bad = "fail"
```

- `match.argv0` is required; `argv_contains` is optional.
- Injection appends missing arguments once; they are never duplicated.
- `format` supports `json` and `jsonl`.
- Rule files must be regular (non-symlink) TOML files, size-limited and validated; invalid rules are ignored with a startup warning.
- Project rules are loaded only for trusted roots. Rules never override built-in parsers for the same argv0 unless priority is lower.
- Declarative parsers cannot execute code. WASM parsers are deferred to v2.
