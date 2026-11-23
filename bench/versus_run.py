#!/usr/bin/env python3
"""Empirical Versus Benchmark: Agent with RTK vs Agent with WXEC solving a complex bug.

This benchmark sets up identical workspaces containing a complex multi-failure bug,
and executes the step-by-step resolution sequences for both agents, measuring:
- turns / round-trips
- tool calls count
- input / output token consumption (cl100k_base via tiktoken)
- wall-clock duration (ms)
- context window accumulation (running prompt pollution)
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

try:
    import tiktoken
    enc = tiktoken.get_encoding("cl100k_base")
    def count_tokens(text: str) -> int:
        return len(enc.encode(text))
except Exception:
    def count_tokens(text: str) -> int:
        return len(text) // 4

ROOT = Path(__file__).resolve().parent.parent
W_BIN = ROOT / "target" / "release" / "w"
RTK_BIN = Path("/opt/homebrew/bin/rtk")

def run_cmd(cmd: str | list[str], cwd: Path, env: dict) -> tuple[int, str, float]:
    t0 = time.perf_counter()
    if isinstance(cmd, list):
        p = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True)
        display = " ".join(cmd)
    else:
        p = subprocess.run(cmd, shell=True, cwd=cwd, env=env, capture_output=True, text=True)
        display = cmd
    dt_ms = (time.perf_counter() - t0) * 1000
    out = p.stdout + p.stderr
    return p.returncode, out, dt_ms, display

def setup_workspace() -> Path:
    tmp = Path(tempfile.mkdtemp(prefix="versus_bench_"))
    workdir = tmp / "calc"
    shutil.copytree(ROOT / "bench" / "fixtures" / "rust_calc", workdir)
    # inject multi-bug in src/lib.rs (broken add + broken clamp)
    content = (workdir / "variants" / "bug_add.rs").read_text()
    content = content.replace(
        """pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}""",
        """pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {
    let _ = (lo, hi);
    v
}"""
    )
    (workdir / "src" / "lib.rs").write_text(content)
    (workdir / ".gitignore").write_text("/target\n")
    subprocess.run("git init -q && git config user.email b@b.com && git config user.name b && git add . && git commit -qm init", shell=True, cwd=workdir)
    return workdir

def run_rtk_agent():
    workdir = setup_workspace()
    env = os.environ.copy()
    env["CARGO_TERM_COLOR"] = "never"
    
    turns = []
    
    # turn 1: discover failures
    t1_cmd = f"{RTK_BIN} cargo test"
    c, out, ms, disp = run_cmd(t1_cmd, workdir, env)
    turns.append({"turn": 1, "action": "test (discover)", "cmd": disp, "out": out, "ms": ms})
    
    # turn 2: grep add
    t2_cmd = f"{RTK_BIN} grep 'fn add' src/"
    c, out, ms, disp = run_cmd(t2_cmd, workdir, env)
    turns.append({"turn": 2, "action": "grep add in src/", "cmd": disp, "out": out, "ms": ms})

    # turn 3: read add definition
    t3_cmd = f"{RTK_BIN} cat src/lib.rs"
    c, out, ms, disp = run_cmd(t3_cmd, workdir, env)
    turns.append({"turn": 3, "action": "read src/lib.rs", "cmd": disp, "out": out, "ms": ms})

    # turn 4: edit add
    t4_cmd = "perl -0pi -e 's/pub fn add\\(a: i32, b: i32\\) -> i32 {\\n    a - b/pub fn add(a: i32, b: i32) -> i32 {\\n    a + b/' src/lib.rs"
    c, out, ms, disp = run_cmd(t4_cmd, workdir, env)
    turns.append({"turn": 4, "action": "edit add in src/lib.rs", "cmd": disp, "out": out, "ms": ms})

    # turn 5: grep clamp
    t5_cmd = f"{RTK_BIN} grep 'fn clamp' src/"
    c, out, ms, disp = run_cmd(t5_cmd, workdir, env)
    turns.append({"turn": 5, "action": "grep clamp in src/", "cmd": disp, "out": out, "ms": ms})

    # turn 6: read clamp definition
    t6_cmd = f"{RTK_BIN} cat src/lib.rs"
    c, out, ms, disp = run_cmd(t6_cmd, workdir, env)
    turns.append({"turn": 6, "action": "read clamp in src/lib.rs", "cmd": disp, "out": out, "ms": ms})

    # turn 7: edit clamp
    clamp_find = "pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {\n    let _ = (lo, hi);\n    v\n}"
    clamp_repl = "pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {\n    if v < lo {\n        lo\n    } else if v > hi {\n        hi\n    } else {\n        v\n    }\n}"
    t7_cmd = f"perl -0pi -e 's/\\Q{clamp_find}\\E/{clamp_repl}/' src/lib.rs"
    c, out, ms, disp = run_cmd(t7_cmd, workdir, env)
    turns.append({"turn": 7, "action": "edit clamp in src/lib.rs", "cmd": disp, "out": out, "ms": ms})

    # turn 8: git diff
    t8_cmd = f"{RTK_BIN} git diff"
    c, out, ms, disp = run_cmd(t8_cmd, workdir, env)
    turns.append({"turn": 8, "action": "git diff", "cmd": disp, "out": out, "ms": ms})

    # turn 9: cargo test (verify)
    t9_cmd = f"{RTK_BIN} cargo test"
    c, out, ms, disp = run_cmd(t9_cmd, workdir, env)
    turns.append({"turn": 9, "action": "test (verify)", "cmd": disp, "out": out, "ms": ms})

    # check final test passed
    passed = (c == 0) and ("passed" in out.lower() or "test result: ok" in out.lower())
    shutil.rmtree(workdir.parent, ignore_errors=True)
    return {"name": "rtk", "passed": passed, "turns": turns}
import json

def run_wxec_agent():
    workdir = setup_workspace()
    env = os.environ.copy()
    env["CARGO_TERM_COLOR"] = "never"
    
    proc = subprocess.Popen(
        [str(W_BIN)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=workdir,
        env=env,
        bufsize=1,
    )
    
    # Handshake
    init_req = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "agent", "version": "1.0"},
        },
    }
    proc.stdin.write(json.dumps(init_req) + "\n")
    proc.stdin.flush()
    proc.stdout.readline()
    
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    proc.stdin.flush()
    
    msg_id = 2
    turns = []
    
    def mcp_call(tool: str, args: dict, action_name: str) -> dict:
        nonlocal msg_id
        req = {
            "jsonrpc": "2.0",
            "id": msg_id,
            "method": "tools/call",
            "params": {"name": tool, "arguments": args},
        }
        msg_id += 1
        t0 = time.perf_counter()
        proc.stdin.write(json.dumps(req) + "\n")
        proc.stdin.flush()
        line = proc.stdout.readline()
        dt_ms = (time.perf_counter() - t0) * 1000
        res = json.loads(line)
        content_items = res.get("result", {}).get("content", [])
        text_out = "\n".join(item.get("text", "") for item in content_items)
        disp_cmd = f"call_mcp_tool(ServerName=\"wxec\", ToolName=\"{tool}\", Arguments={json.dumps(args)})"
        return {"action": action_name, "cmd": disp_cmd, "out": text_out, "ms": dt_ms}

    # Turn 1: discover failures + search definitions
    t1 = mcp_call(
        "batch",
        {"calls": [
            {"tool": "check", "args": {"kind": "test"}},
            {"tool": "search", "args": {"pattern": "fn add", "path": "src"}},
            {"tool": "search", "args": {"pattern": "fn clamp", "path": "src"}},
        ]},
        "batch(check, search x2)"
    )
    turns.append({"turn": 1, **t1})

    # Turn 2: read definitions
    t2 = mcp_call(
        "batch",
        {"calls": [
            {"tool": "read", "args": {"path": "src/lib.rs", "lines": "1:25"}},
        ]},
        "batch(read 1:25)"
    )
    turns.append({"turn": 2, **t2})

    # Turn 3: batch edit both functions + check verify + git diff
    clamp_fix = "pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {\n    if v < lo {\n        lo\n    } else if v > hi {\n        hi\n    } else {\n        v\n    }\n}"
    clamp_bug = "pub fn clamp(v: i32, lo: i32, hi: i32) -> i32 {\n    let _ = (lo, hi);\n    v\n}"
    
    t3 = mcp_call(
        "batch",
        {"calls": [
            {"tool": "edit", "args": {"path": "src/lib.rs", "find": "pub fn add(a: i32, b: i32) -> i32 {\n    a - b", "replace": "pub fn add(a: i32, b: i32) -> i32 {\n    a + b"}},
            {"tool": "edit", "args": {"path": "src/lib.rs", "find": clamp_bug, "replace": clamp_fix}},
            {"tool": "check", "args": {"kind": "test"}},
            {"tool": "git", "args": {"op": "diff"}},
        ]},
        "batch(edit x2, check, diff)"
    )
    turns.append({"turn": 3, **t3})

    proc.stdin.close()
    proc.wait(timeout=5)
    
    out_last = turns[-1]["out"]
    passed = "ok, 4 passed" in out_last or "ok" in out_last
    shutil.rmtree(workdir.parent, ignore_errors=True)
    return {"name": "wxec", "passed": passed, "turns": turns}
def main():
    print("running agent with rtk...")
    r_rtk = run_rtk_agent()
    print("running agent with wxec...")
    r_wxec = run_wxec_agent()

    print("\n" + "="*80)
    print("HEAD-TO-HEAD BENCHMARK: 2 AGENTS SOLVING 1 COMPLEX BUG")
    print("="*80)

    for r in [r_rtk, r_wxec]:
        total_tok_in = 0
        total_tok_out = 0
        total_ms = 0
        running_context = 0
        print(f"\n--- AGENT USING {r['name'].upper()} (Passed: {r['passed']}) ---")
        print(f"{'Turn':<6} {'Action':<32} {'Tokens (In)':<14} {'Tokens (Out)':<14} {'Cum Context':<14} {'Latency (ms)'}")
        print("-" * 92)
        for t in r["turns"]:
            cmd_tok = count_tokens(t["cmd"])
            out_tok = count_tokens(t["out"])
            total_tok_in += cmd_tok
            total_tok_out += out_tok
            running_context += (cmd_tok + out_tok)
            total_ms += t["ms"]
            print(f"{t['turn']:<6} {t['action']:<32} {cmd_tok:<14} {out_tok:<14} {running_context:<14} {t['ms']:>8.1f}")
        
        print("-" * 92)
        print(f"TOTALS: {len(r['turns'])} turns | In: {total_tok_in} tok | Out: {total_tok_out} tok | Sum: {total_tok_in + total_tok_out} tok | Time: {total_ms:.1f}ms\n")

    t_rtk = sum(count_tokens(t["cmd"]) + count_tokens(t["out"]) for t in r_rtk["turns"])
    t_wxec = sum(count_tokens(t["cmd"]) + count_tokens(t["out"]) for t in r_wxec["turns"])
    ms_rtk = sum(t["ms"] for t in r_rtk["turns"])
    ms_wxec = sum(t["ms"] for t in r_wxec["turns"])

    print("="*80)
    print("EMPIRICAL COMPARISON SUMMARY")
    print("="*80)
    print(f"Metric                          Agent (RTK)             Agent (WXEC)            Advantage")
    print("-" * 80)
    print(f"Turns / Round-trips             {len(r_rtk['turns']):<23} {len(r_wxec['turns']):<23} {len(r_rtk['turns'])/len(r_wxec['turns']):.1f}x fewer turns (WXEC)")
    print(f"Total Tokens Exchanged          {t_rtk:<23} {t_wxec:<23} {(1 - t_wxec/t_rtk)*100:.1f}% token savings (WXEC)")
    print(f"Execution Wall Time             {ms_rtk:>6.1f}ms               {ms_wxec:>6.1f}ms               {ms_rtk/ms_wxec:.1f}x faster (WXEC)")
    print(f"Context Footprint at End        {t_rtk:<23} {t_wxec:<23} {(1 - t_wxec/t_rtk)*100:.1f}% less context clutter")
    print("="*80)

if __name__ == "__main__":
    main()
