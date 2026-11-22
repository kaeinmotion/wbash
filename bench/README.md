# Benchmark harness

Two runners execute the same 30 scripted tasks (`bench/tasks/*.yaml`):

- `bash`: direct `grep`, `sed`, `git`, `cargo`, `perl` pipelines.
- `w`: the same intent expressed with `w` tools (`search`, `read`, `edit`,
  `git`, `run`).

Each task records a success check and, for read-only tasks, expected symbols.
Metrics: success, steps (tool calls), estimated tokens (`(command + output)/4`),
and wall time. Setup and the success check are excluded from steps/tokens.

This is a **scripted harness**, not an agent trial: it isolates tool output cost
for a known navigation/edit strategy. Agentic runs (turns, retries, model tokens)
belong in a separate E2E harness.

## Run

```sh
cargo build --release
python3 bench/run.py --runners bash,w
python3 bench/run.py --task fix_add --runner w --verbose
python3 bench/run.py --save-baseline bench/baselines/v0.1.json
```

The runner exits nonzero if any task fails. `bench/results/` is ignored;
checked baselines live in `bench/baselines/`.

## Task mix

- 8 locate-symbol, 5 explain-module, 3 summarize-diff (read-only).
- 6 fix-failing-test, 4 rename-symbol, 4 lint/type/syntax repairs.
- Kinds are tagged in each YAML `kind` field.

## Baseline v0.1 (macOS arm64, debug `w`)

| runner | passed | steps | tokens (est) | wall |
|---|---|---|---|---|
| bash | 30/30 | 52 | 11,545 | 18.7s |
| w | 30/30 | 61 | 4,977 | 20.0s |

`w` uses 57% fewer output tokens and slightly more calls (handles + explicit
reads); wall time is dominated by identical `cargo test` work. Re-run on Linux
CI for comparable absolute numbers.

## Latency smoke (G6-G8)

```sh
cargo build --release
python3 bench/perf.py             # advisory, prints gates
python3 bench/perf.py --strict    # nonzero on misses
```

`perf.py` creates a large workspace and measures cold start, read, and search
per CLI invocation (advisory; process startup/no durable warm index affect G6–G8).
Warm in-process read/search gates are enforced via
`crates/wxec-tools/tests/warm_perf.rs` (`bench/warm_gate.py`, CI `perf-smoke`).
See `docs/AMENDMENTS.md` A12–A13.
