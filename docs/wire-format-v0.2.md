# wire format specification (lean@0.2)

formal specification for the wxec model context protocol (mcp) wire protocol, token estimation heuristics, and handle store mechanics.

---

### **table of contents**

- [chapter 1: design principles & wire grammar](#chapter-1-design-principles--wire-grammar)
- [chapter 2: record types & compression semantics](#chapter-2-record-types--compression-semantics)
- [chapter 3: deterministic token estimation & budget fitting](#chapter-3-deterministic-token-estimation--budget-fitting)
- [chapter 4: json serialization format](#chapter-4-json-serialization-format)
- [chapter 5: wire protocol examples](#chapter-5-wire-protocol-examples)

---

### **chapter 1: design principles & wire grammar**

the wxec wire format is engineered for maximum token density while preserving actionable error messages and stack traces.

#### **formal grammar (ebnf)**
```text
response    := header LF body?
header      := summary
body        := (record LF)* record?
record      := content | elision | advisory | error | diagnostic
elision     := "...+" integer SP ("lines" | "bytes" | "symbols") SP "(" handle ")"
advisory    := "# " text
error       := "!" error_code SP message ("." SP hint)?
handle      := "~" [a-z0-9]{1,8}
```

#### **invariants**
- utf-8 encoded, newline-separated (`\n` only), with a guaranteed trailing newline.
- zero ansi/csi escape codes; zero trailing whitespace; zero redundant blank lines.
- line 1 is universally reserved for count-bearing summary headers.

---

### **chapter 2: record types & compression semantics**

1. **headers**: line 1 always states the high-level outcome:
   - clean read: `path/to/file (N lines, b3:hash)`
   - unchanged verification: `unchanged path/to/file (b3:hash) expand ~id`
   - search: `N files, M matches`
   - check/run: `ok, N passed, M warnings` or `exit 1, N failed`
2. **elisions**: omitted sections are explicitly cataloged: `...+N lines (~id)`. every elision points to an addressable handle in the lossless store.
3. **must-keep records**: compilation errors, failed assertions, panics, and security alerts bypass budget truncations. when token budgets are tight, ambient lines are dropped, but critical failure paths are preserved.

---

### **chapter 3: deterministic token estimation & budget fitting**

to prevent sudden context window overflow, wxec computes deterministic token weights:

$$\text{estimated tokens} = \max\left(\left\lceil\frac{\text{characters}}{4}\right\rceil, \left\lfloor\frac{\text{lines}}{2}\right\rfloor\right)$$

#### **default token budgets**
- standard inspection tools (`read`, `search`, `tree`): **600 tokens**.
- execution and compilation tools (`check`, `run`, `sh`): **800 tokens**.
- composite batch operations (`batch`): **1600 tokens**.
- explicit limits: any client can supply `budget: N` within the bounds `64..=32768`.

---

### **chapter 4: json serialization format**

when invoked with `format: "json"`, wxec emits a normalized envelope:

```json
{
  "ok": true,
  "tool": "search",
  "meta": {
    "duration_ms": 11,
    "counts": {
      "files": 2,
      "matches": 4
    },
    "redactions": 0,
    "notes": []
  },
  "data": {
    "matches": [
      {
        "file": "crates/wxec-core/src/lib.rs",
        "line": 42,
        "content": "pub fn execute() -> Result<()> {"
      }
    ]
  },
  "diagnostics": [],
  "handles": [
    {
      "ref": "~s1",
      "kind": "search",
      "bytes": 2048,
      "hash": "b3:4f8e..."
    }
  ],
  "errors": []
}
```

---

### **chapter 5: wire protocol examples**

#### **directory tree inspection (`tree`)**
```text
./ (4 files, 2 dirs)
src/
  lib.rs
  main.rs
tests/
  integration.rs
Cargo.toml
```

#### **initial file read (`read`)**
```text
src/lib.rs (4 lines, b3:9a12c8e)
1: pub fn add(a: i32, b: i32) -> i32 {
2:     a + b
3: }
```

#### **verified unchanged read (session ledger hit)**
```text
unchanged src/lib.rs (b3:9a12c8e) expand ~h101
```

#### **search results (`search`)**
```text
2 files, 2 matches
src/lib.rs:1: pub fn add(a: i32, b: i32) -> i32 {
tests/integration.rs:5: assert_eq!(add(2, 2), 4);
```

#### **test failure diagnostic (`check kind="test"`)**
```text
exit 101, 38ms, 1 passed, 1 failed
test result: FAILED. 1 passed; 1 failed
FAIL tests::test_overflow
assertion `left == right` failed: left: 4, right: 5
full: ~r12
```
