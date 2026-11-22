#!/usr/bin/env python3
"""Latency smoke for spec gates G6-G8.

Generates a temporary workspace (default 50k small files), then measures:
  cold   `w --version` (process start+config load)
  read   `w read target.rs` (CLI per-invocation; includes process startup)
  search `w search needle` (after warm-up to exercise durable cross-process cache)

Spec thresholds: cold <50ms, read <5ms, search <50ms. CLI `read` includes process
startup so G6 is primarily validated for warm MCP/in-process transport; `search`
uses the persisted workspace/index+content sidecar when warm. CI treats this as
advisory and enforces warm in-process gates separately (see bench/warm_gate.py).
Use --strict to fail the run on any miss.
"""

from __future__ import annotations

import argparse
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path


def p95(values: list[float]) -> float:
    values = sorted(values)
    return values[max(0, int(len(values) * 0.95) - 1)]


def timed(cmd: list[str], cwd: Path, env: dict, runs: int) -> list[float]:
    times = []
    for _ in range(runs):
        start = time.perf_counter()
        subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True)
        times.append((time.perf_counter() - start) * 1000.0)
    return times


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--w-bin", default=str(Path(__file__).resolve().parent.parent / "target/release/w"))
    ap.add_argument("--files", type=int, default=50_000)
    ap.add_argument("--cold-runs", type=int, default=20)
    ap.add_argument("--read-runs", type=int, default=30)
    ap.add_argument("--search-runs", type=int, default=3)
    ap.add_argument("--strict", action="store_true")
    ap.add_argument("--keep", action="store_true")
    args = ap.parse_args()

    w = Path(args.w_bin).resolve()
    if not w.exists():
        print(f"missing binary: {w}", file=sys.stderr)
        return 2
    root = Path(tempfile.mkdtemp(prefix="wperf-"))
    env = os.environ.copy()
    env["XDG_CACHE_HOME"] = str(root / ".cache")
    env["XDG_CONFIG_HOME"] = str(root / ".config")
    env["WXEC_ALLOW_CLI"] = "1"
    try:
        per_dir = 250
        dirs = max(1, args.files // per_dir)
        for d in range(dirs):
            folder = root / f"d{d:04}"
            folder.mkdir()
            for f in range(per_dir):
                (folder / f"f{f:03}.rs").write_text("// filler\n")
        (root / "target.rs").write_text("\n".join(f"line {i} needle" for i in range(40)) + "\n")

        cold = timed([str(w), "--version"], root, env, args.cold_runs)
        # warm-up builds persistent cache/sidecar; measured runs reflect warm CLI behavior
        subprocess.run([str(w), "search", "needle"], cwd=root, env=env, capture_output=True, text=True)
        subprocess.run([str(w), "read", "target.rs"], cwd=root, env=env, capture_output=True, text=True)
        read = timed([str(w), "read", "target.rs"], root, env, args.read_runs)
        search = timed([str(w), "search", "needle"], root, env, args.search_runs)

        rows = [
            ("cold", args.cold_runs, 50.0, cold),
            ("read", args.read_runs, 5.0, read),
            ("search", args.search_runs, 50.0, search),
        ]
        print(f"workspace: {dirs * per_dir} files under {root}")
        print(f"{'gate':<7} {'n':>3} {'median':>9} {'p95':>9} {'limit':>7}  status")
        missed = []
        for name, n, limit, times in rows:
            med, p = statistics.median(times), p95(times)
            ok = p <= limit
            if not ok:
                missed.append(name)
            print(f"{name:<7} {n:>3} {med:>8.2f}ms {p:>8.2f}ms {limit:>6.1f}ms  {'ok' if ok else 'MISS'}")
        if missed:
            print(f"\nmissed gates: {', '.join(missed)} (see docs/AMENDMENTS.md A13)")
            if "read" in missed and "search" not in missed:
                print("note: CLI read includes process startup; warm transport target validated by in-process gate")
        return 1 if (args.strict and missed) else 0
    finally:
        if not args.keep:
            shutil.rmtree(root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
