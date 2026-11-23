# Benchmark report

Environment: macOS arm64, rustc 1.95, release `w`, local APFS.
Token metrics: chars/4 and `tiktoken cl100k_base` (durations stripped from output for stability).

## Scripted baseline v0.2.3 (30 tasks)

`python3 bench/run.py --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
|---|---:|---:|---:|---:|---:|
| bash | 30/30 | 52 | 11,566 | 11,986 | 17.6s |
| w | 30/30 | 43 | 2,870 | 3,821 | 18.2s |

- Token delta (w vs bash): **-75.2%** (chars/4), **-68.1%** (cl100k).
- Correctness: 30/30 for both strategies.

### By kind (cl100k)

| kind | bash | w | delta | bash steps | w steps |
|---|---:|---:|---:|---:|---:|
| explain | 9,055 | 1,492 | -83.5% | 10 | 5 |
| fix_failing_test | 1,114 | 1,092 | -2.0% | 12 | 12 |
| fix_lint_or_type | 684 | 403 | -41.1% | 8 | 8 |
| locate | 483 | 459 | -5.0% | 8 | 8 |
| rename_symbol | 296 | 176 | -40.5% | 8 | 4 |
| summarize_diff | 354 | 199 | -43.8% | 6 | 6 |

Changes from v0.2.2: search literal `|` auto-retries safe alternation with notes, CLI accepts `-e/--regexp` and rejects unknown short flags, tree inlines small leaf dirs and places handle ids on collapsed labels, read emits head before outline with no duplicate in-head symbols, git headers use `N changed`, expand enforces `handles.expand_max_tokens` with `remaining`/`next_offset`, and panic/test failure output preserves assertion details.

## Agent benchmark (6 multi-step tasks)

`python3 bench/run.py --tasks-dir bench/tasks-agent --runners bash,w --w-bin target/release/w`

| runner | passed | steps | tok/4 | cl100k | wall |
|---|---:|---:|---:|---:|---:|
| bash | 6/6 | 19 | 1,228 | 1,724 | 6.1s |
| w | 6/6 | 18 | 660 | 936 | 6.3s |

- Token delta (w vs bash): **-46.3%** (chars/4), **-45.7%** (cl100k).
- Tasks: fix failing test, fix compile error, rename across files, clippy fix, git edit/summarize, parse bug fix.

## Latency

Warm in-process (gated, release): read p95 ~0.23ms, search p95 ~10.48ms @50k small files; CI gate defaults read <5ms, search <25ms @10k (`bench/warm_gate.py`).

CLI per-invocation remains startup-bound for reads (~6-7ms p95 advisory); warmed cross-process search typically ~15-25ms depending on fixture/FS (`bench/perf.py`).

## Gates and CI

- `bench-scripted` runs with `--baseline bench/baselines/v0.2.3-baseline.json --max-regress-pct 10`.
- Baselines saved: `bench/baselines/v0.2.3-baseline.json`, `bench/baselines/agent-current.json`.
- Warm perf, fmt/clippy, tests, MCP schema budget, estimator calibration, security, and fuzz jobs remain in CI.

## Versus: RTK vs WXEC on Complex Multi-Failure Bug

Empirical head-to-head comparison between Agent with `rtk` (Rust Token Killer) and Agent with `wxec` (native MCP stdio batching) on identical isolated workspaces:

| metric | agent with `rtk` | agent with `wxec` (mcp) | measured advantage |
|---|---:|---:|---|
| **turns / round-trips** | 9 | 3 | **3.0x fewer turns** |
| **total tokens exchanged (cl100k)** | 1,393 | 909 | **34.7% token savings** |
| **context footprint at end** | 1,393 | 909 | **34.7% cleaner context** |
| **execution wall time** | 2,545.6ms | 2,212.2ms | **1.15x faster** |
| **llm inference latency (at ~1.8s/turn)** | ~16.2s | ~5.4s | **3.0x faster task resolution** |

*full reproduction script: `bench/versus_run.py`; comprehensive writeup: `docs/benchmarks.md`.*
## Gaps

- Agent benchmarks are scripted multi-step fixture tasks, not live model E2E turn/retry measurements.
- Cross-process CLI read latency remains startup-bound; further gains require daemon/MCP warm transport or deeper persistent hot cache.
- 10-bug trial logs for call-count/expand-loop analysis were not available in-repo; expand cap fix is config-enforced and may need follow-up with real traces.
