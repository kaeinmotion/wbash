#!/usr/bin/env python3
"""Scripted benchmark harness: plain bash vs w tools.

Tasks are single-line YAML records under bench/tasks. Each task names a fixture
(optional), setup commands, a success check, and equivalent step lists for the
`bash` and `w` runners. Steps are shell commands; setup and the success check
are not counted as tool calls or tokens.

Usage:
  python3 bench/run.py --runners w,bash
  python3 bench/run.py --task fix_add --runner w
  python3 bench/run.py --save-baseline bench/baselines/v0.1.json
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

try:
    import tiktoken
    _ENC = tiktoken.get_encoding("cl100k_base")
except Exception:
    _ENC = None


import re as _re

_DUR_RE = _re.compile(r", \d+(?:\.\d+)?(?:ms|s|m)\b")


def normalize_output_for_tokens(out: str) -> str:
    return _DUR_RE.sub("", out)


def count_tokens(cmd: str, out: str) -> tuple[int, int]:
    out = normalize_output_for_tokens(out)
    est = (len(cmd) + len(out)) // 4
    if _ENC is not None:
        try:
            real = len(_ENC.encode(cmd + "\n" + out))
            return est, real
        except Exception:
            pass
    return est, est

BENCH = Path(__file__).resolve().parent
ROOT = BENCH.parent


def scalar(value: str):
    if value.startswith(('"', "'")):
        return json.loads(value) if value.startswith('"') else value[1:-1]
    if value in ("true", "false"):
        return value == "true"
    return value


def parse_yaml(path: Path) -> dict:
    task: dict = {}
    current = None
    for raw in path.read_text().splitlines():
        line = raw.rstrip()
        if not line or line.lstrip().startswith("#"):
            continue
        if line.startswith("  - "):
            if current is None:
                raise ValueError(f"{path}: list item without key")
            item = line[4:].strip()
            task.setdefault(current, []).append(scalar(item))
            continue
        if line.startswith(" "):
            raise ValueError(f"{path}: unsupported indentation: {line!r}")
        key, _, value = line.partition(":")
        key = key.strip()
        value = value.strip()
        if value == "":
            current = key
            task.setdefault(key, [])
        else:
            current = None
            task[key] = scalar(value)
    return task


def load_tasks(pattern: str | None, tasks_dir: Path | None = None) -> list[dict]:
    base = tasks_dir or (BENCH / "tasks")
    tasks = []
    for path in sorted(base.glob("*.yaml")):
        task = parse_yaml(path)
        task["_path"] = str(path)
        if pattern and pattern not in task["id"]:
            continue
        tasks.append(task)
    return tasks


def run_shell(cmd: str, cwd: Path, env: dict, timeout: int) -> tuple[int, str, int]:
    started = time.perf_counter()
    try:
        proc = subprocess.run(
            cmd, shell=True, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout
        )
        code, out = proc.returncode, proc.stdout + proc.stderr
    except subprocess.TimeoutExpired as exc:
        code = 124
        out = (exc.stdout or "") + (exc.stderr or "") + "\n[bench] timeout\n"
        if isinstance(out, bytes):
            out = out.decode("utf-8", "replace")
    return code, out, int((time.perf_counter() - started) * 1000)


def prepare(task: dict, w_bin: Path, timeout: int) -> tuple[Path, dict, Path | None]:
    temp = Path(tempfile.mkdtemp(prefix="wbench-"))
    fixture = task.get("fixture")
    if fixture:
        workdir = temp / "work"
        shutil.copytree(BENCH / fixture, workdir, ignore=shutil.ignore_patterns(".git"))
    else:
        workdir = ROOT
    env = os.environ.copy()
    env["WXEC_ALLOW_CLI"] = "1"
    cargo_bin = Path.home() / ".cargo" / "bin"
    path_parts = [str(w_bin.parent)]
    if cargo_bin.is_dir():
        path_parts.append(str(cargo_bin))
    path_parts.append(env.get("PATH", ""))
    env["PATH"] = ":".join(p for p in path_parts if p)
    env["XDG_CACHE_HOME"] = str(temp / "cache")
    env["XDG_CONFIG_HOME"] = str(temp / "config")
    env["CARGO_TARGET_DIR"] = str(BENCH / ".cache" / "cargo-target")
    env["CARGO_TERM_COLOR"] = "never"
    for cmd in task.get("setup", []):
        code, out, _ = run_shell(cmd, workdir, env, timeout)
        if code != 0:
            return workdir, env, temp
    # Keep fixture setup times out of the measured window.
    return workdir, env, temp


def run_task(task: dict, runner: str, w_bin: Path, timeout: int, verbose: bool = False) -> dict:
    workdir, env, temp = prepare(task, w_bin, timeout)
    started = time.perf_counter()
    codes, outputs, tokens = [], [], 0
    tokens_real = 0
    for cmd in task.get(runner, []):
        code, out, _ = run_shell(cmd, workdir, env, timeout)
        codes.append(code)
        outputs.append(out)
        est, real = count_tokens(cmd, out)
        tokens += est
        tokens_real += real
        if verbose:
            print(f"    $ {cmd}\n{out[:2000]}")
    check = task.get("check")
    check_code = 0
    if check:
        check_code, check_out, _ = run_shell(check, workdir, env, timeout)
        outputs.append(check_out)
    wall_ms = int((time.perf_counter() - started) * 1000)
    if verbose and check:
        print(f"    check: {check} -> {check_code}\n{outputs[-1][:2000]}")
    combined = "\n".join(outputs)
    missing = [needle for needle in task.get("expect", []) if needle not in combined]
    ok = check_code == 0 and not missing
    result = {
        "task": task["id"],
        "kind": task.get("kind", ""),
        "runner": runner,
        "ok": ok,
        "steps": len(task.get(runner, [])),
        "step_exit_codes": codes,
        "check_exit": check_code,
        "missing_expect": missing,
        "tokens_est": tokens,
        "tokens_cl100k": tokens_real,
        "wall_ms": wall_ms,
    }
    if temp is not None:
        shutil.rmtree(temp, ignore_errors=True)
    return result


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runners", default="bash,w")
    ap.add_argument("--runner", choices=["bash", "w"])
    ap.add_argument("--task", default=None)
    ap.add_argument("--tasks-dir", default=None)
    ap.add_argument("--w-bin", default=None)
    ap.add_argument("--timeout", type=int, default=180)
    ap.add_argument("--save-baseline", default=None)
    ap.add_argument("--baseline", default=None)
    ap.add_argument("--max-regress-pct", type=float, default=None)
    ap.add_argument("--json", default=None)
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args()

    tasks_dir = Path(args.tasks_dir) if args.tasks_dir else None
    tasks = load_tasks(args.task, tasks_dir)
    if args.list:
        for t in tasks:
            print(f"{t['id']:<24} {t.get('kind','')}")
        return 0
    if not tasks:
        print("no tasks matched", file=sys.stderr)
        return 2
    w_default = ROOT / "target" / "debug" / "w"
    if not w_default.exists():
        w_default = ROOT / "target" / "release" / "w"
    w_bin = Path(args.w_bin or w_default).resolve()
    if "w" in (args.runner or args.runners) and not w_bin.exists():
        print(f"missing w binary at {w_bin}; run `cargo build` first", file=sys.stderr)
        return 2
    runners = [args.runner] if args.runner else [r for r in args.runners.split(",") if r]

    results = []
    print(f"{'task':<24} {'runner':<6} {'ok':<3} {'steps':>5} {'tok/4':>7} {'cl100k':>7} {'ms':>6}")
    for task in tasks:
        for runner in runners:
            r = run_task(task, runner, w_bin, args.timeout, args.verbose)
            results.append(r)
            print(
                f"{r['task']:<24} {r['runner']:<6} {str(r['ok']):<3} "
                f"{r['steps']:>5} {r['tokens_est']:>7} {r['tokens_cl100k']:>7} {r['wall_ms']:>6}"
            )

    summary = {}
    for runner in runners:
        rows = [r for r in results if r["runner"] == runner]
        summary[runner] = {
            "tasks": len(rows),
            "passed": sum(1 for r in rows if r["ok"]),
            "tokens_est": sum(r["tokens_est"] for r in rows),
            "tokens_cl100k": sum(r.get("tokens_cl100k", r["tokens_est"]) for r in rows),
            "steps": sum(r["steps"] for r in rows),
            "wall_ms": sum(r["wall_ms"] for r in rows),
            "by_kind": {},
        }
        kinds = sorted({r.get("kind", "") for r in rows})
        for k in kinds:
            krows = [r for r in rows if r.get("kind", "") == k]
            summary[runner]["by_kind"][k] = {
                "tasks": len(krows),
                "passed": sum(1 for r in krows if r["ok"]),
                "tokens_est": sum(r["tokens_est"] for r in krows),
                "tokens_cl100k": sum(r.get("tokens_cl100k", r["tokens_est"]) for r in krows),
                "steps": sum(r["steps"] for r in krows),
            }
    print("\nsummary")
    for runner, ssum in summary.items():
        print(
            f"  {runner:<6} passed={ssum['passed']}/{ssum['tasks']} "
            f"tok/4={ssum['tokens_est']} cl100k={ssum['tokens_cl100k']} steps={ssum['steps']} ms={ssum['wall_ms']}"
        )
        for k, ks in ssum.get("by_kind", {}).items():
            if not k:
                continue
            print(
                f"    - {k:<16} passed={ks['passed']}/{ks['tasks']} tok/4={ks['tokens_est']} cl100k={ks['tokens_cl100k']} steps={ks['steps']}"
            )
    if "bash" in summary and "w" in summary:
        if summary["bash"]["tokens_est"]:
            delta = 100 * (summary["w"]["tokens_est"] - summary["bash"]["tokens_est"]) / summary["bash"]["tokens_est"]
            print(f"  token delta w vs bash (tok/4): {delta:+.1f}%")
        if summary["bash"].get("tokens_cl100k"):
            delta_r = 100 * (summary["w"]["tokens_cl100k"] - summary["bash"]["tokens_cl100k"]) / summary["bash"]["tokens_cl100k"]
            print(f"  token delta w vs bash (cl100k): {delta_r:+.1f}%")

    payload = {"generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "tokenizer": "cl100k_base" if _ENC else "chars/4", "summary": summary, "results": results}
    if args.save_baseline:
        out = Path(args.save_baseline)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(payload, indent=2) + "\n")
        print(f"baseline written to {out}")
    if args.json:
        out = Path(args.json)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(payload, indent=2) + "\n")

    if args.baseline and args.max_regress_pct is not None:
        base = json.loads(Path(args.baseline).read_text())
        base_w = base.get("summary", {}).get("w", {})
        cur_w = summary.get("w", {})
        use_cl100k = base.get("tokenizer") == "cl100k_base" and payload.get("tokenizer") == "cl100k_base"
        metric = "tokens_cl100k" if use_cl100k else "tokens_est"
        base_total = base_w.get(metric)
        cur_total = cur_w.get(metric)
        if base_total and cur_total:
            delta = 100.0 * (cur_total - base_total) / base_total
            print(f"  baseline regression w {metric}: {delta:+.1f}% (max {args.max_regress_pct:+.1f}%)")
            if delta > args.max_regress_pct:
                print("  FAIL: token regression above threshold", file=sys.stderr)
                return 1
        base_by_id = {r["task"]: r for r in base.get("results", []) if r.get("runner") == "w"}
        for r in results:
            if r.get("runner") != "w":
                continue
            b = base_by_id.get(r["task"])
            if not b:
                continue
            bt = b.get(metric) or 0
            ct = r.get(metric) or 0
            if bt > 0:
                d = 100.0 * (ct - bt) / bt
                if d > args.max_regress_pct:
                    print(f"  FAIL: {r['task']} {d:+.1f}% vs baseline", file=sys.stderr)
                    return 1

    return 0 if all(r["ok"] for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
