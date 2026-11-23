# benchmarks & empirical comparisons

this document records empirical benchmarks comparing `wxec` against standard bash workflows and alternative token optimization tools like `rtk` (rust token killer).

all measurements were conducted locally on macos arm64 using release binaries (`target/release/w`, `/opt/homebrew/bin/rtk`), tokenized using `tiktoken` (`cl100k_base`), and timed via nanosecond monotonic clocks.

---

## 1. head-to-head versus: `rtk` vs `wxec` (complex bug resolution)

### scenario description
a multi-failure regression was injected into an isolated rust package (`calc`):
1. **logic bug in `src/lib.rs`**: `pub fn add` subtracting instead of adding (`a - b`).
2. **boundary bug in `src/lib.rs`**: `pub fn clamp` failing to clamp bounds and returning raw `v`.
3. **test suite**: unit tests and integration tests in `tests/basic.rs` failing on arithmetic and boundary suites.

both agents resolved all bugs to a 100% passing test suite (`cargo test: ok, 4 passed`).

---

### step-by-step turn execution traces

#### agent 1: `rtk` (cli proxy / command-level interceptor)
agent 1 uses raw shell commands intercepted and filtered through `/opt/homebrew/bin/rtk`.

| turn | action | command executed | tokens in | tokens out | cumulative context | latency |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: |
| **1** | test (discover) | `rtk cargo test` | 9 | 336 | 345 | 1425.1ms |
| **2** | grep `add` | `rtk grep 'fn add' src/` | 14 | 36 | 395 | 47.1ms |
| **3** | read `add` | `rtk cat src/lib.rs` | 11 | 199 | 605 | 25.9ms |
| **4** | edit `add` | `perl -0pi -e 's/.../...' src/lib.rs` | 57 | 0 | 662 | 11.4ms |
| **5** | grep `clamp` | `rtk grep 'fn clamp' src/` | 14 | 41 | 717 | 19.3ms |
| **6** | read `clamp` | `rtk cat src/lib.rs` | 11 | 199 | 927 | 21.7ms |
| **7** | edit `clamp` | `perl -0pi -e 's/.../...' src/lib.rs` | 102 | 0 | 1029 | 10.8ms |
| **8** | diff check | `rtk git diff` | 9 | 201 | 1239 | 40.0ms |
| **9** | test (verify) | `rtk cargo test` | 9 | 143 | 1391 | 846.5ms |

- **total turns**: 9 turns
- **total tokens**: 1,391 tokens (in: 236, out: 1,155)
- **final context footprint**: 1,391 tokens
- **wall time**: 2,447.8ms

---

#### agent 2: `wxec` (native batched mcp server)
agent 2 uses `wxec`'s native multi-tool batching over stdio / cli.

| turn | action | operations batched in turn | tokens in | tokens out | cumulative context | latency |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: |
| **1** | discover & search | `batch("check test", "search 'fn add'", "search 'fn clamp'")` | 27 | 105 | 132 | 842.5ms |
| **2** | targeted read | `batch("read src/lib.rs 1:25")` | 21 | 265 | 418 | 22.5ms |
| **3** | edit, check & diff | `batch(edit add, edit clamp, "check test", "git diff")` | 164 | 166 | 748 | 921.0ms |

- **total turns**: 3 turns
- **total tokens**: 748 tokens (in: 212, out: 536)
- **final context footprint**: 748 tokens
- **wall time**: 1,786.0ms

---

### empirical comparison summary

| metric | agent with `rtk` | agent with `wxec` | measured advantage |
| :--- | :--- | :--- | :--- |
| **turns / round-trips** | **9 turns** | **3 turns** | **3.0x fewer turns** |
| **total tokens exchanged** | **1,391 tokens** | **748 tokens** | **46.2% token savings** |
| **context footprint at end** | **1,391 tokens** | **748 tokens** | **46.2% cleaner context** |
| **execution wall time** | **2,447.8ms** | **1,786.0ms** | **1.37x faster local execution** |
| **llm inference latency (at ~1.5s/turn)** | ~13.5s | ~4.5s | **~3.0x faster task completion** |

---

### architectural analysis: why `wxec` wins

1. **heterogeneous multi-tool batching**:
   - `rtk` operates as a command filter on individual shell steps. every command requires an independent llm api turn, network payload serialization, and model inference cycle.
   - `wxec` bundles test checking, code searching, targeted reading, atomic editing, and diff inspection into single round-trips. reducing turns from 9 to 3 saves not only local execution time but 6 complete llm generation loops (~9 to 18 seconds of model wait time).

2. **token budget fitting & lean wire format**:
   - while `rtk` compresses stdout streams (e.g. cargo test from 800 to 143 tokens), it cannot deduplicate repeated file reads or compress tool call headers across turns.
   - `wxec` renders output using its lean wire format (`#1 check ok`, `#2 search ok`), minimizing token overhead per result to single digits.

3. **atomic anchor editing vs brittle shell regex**:
   - during the benchmark, the `rtk` agent required custom regex escaping for perl/sed (`\Q...\E`), which frequently fails when modifying code with parentheses, closures, or multi-line blocks.
   - `wxec edit` performs exact AST and whitespace-tolerant fuzzy matching. if an anchor is ambiguous, it cleanly halts with line-number hints instead of corrupting source files.

4. **context window longevity**:
   - by maintaining a 46.2% lower cumulative context footprint, `wxec` leaves significantly more token headroom for agent reasoning, avoiding premature context truncation or compaction on long sessions.

---

## 2. scripted task baselines (30 core tasks)

`python3 bench/run.py --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
| :--- | ---: | ---: | ---: | ---: | ---: |
| **bash** | 30/30 | 52 | 11,566 | 11,986 | 17.6s |
| **wxec (`w`)** | 30/30 | 43 | 2,870 | 3,821 | 18.2s |

- **token delta (wxec vs bash)**: **-75.2%** (chars/4), **-68.1%** (cl100k).
- **breakdown by category (cl100k)**:
  - `explain`: **-83.5%** tokens (9,055 $\rightarrow$ 1,492)
  - `rename_symbol`: **-40.5%** tokens (296 $\rightarrow$ 176)
  - `summarize_diff`: **-43.8%** tokens (354 $\rightarrow$ 199)
  - `fix_lint_or_type`: **-41.1%** tokens (684 $\rightarrow$ 403)
