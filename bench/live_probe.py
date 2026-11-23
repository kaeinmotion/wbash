#!/usr/bin/env python3
"""Live Side-by-Side Probe: Raw Standard Execution vs WXEC MCP on this active repository."""

import json
import subprocess
import time
from pathlib import Path

import tiktoken

enc = tiktoken.get_encoding("cl100k_base")

def count_tok(s: str) -> int:
    return len(enc.encode(s))

ROOT = Path(__file__).resolve().parent.parent
W_BIN = ROOT / "target" / "release" / "w"

def run_raw(cmd: str) -> tuple[str, float]:
    t0 = time.perf_counter()
    p = subprocess.run(cmd, shell=True, cwd=ROOT, capture_output=True, text=True)
    dt = (time.perf_counter() - t0) * 1000
    return p.stdout + p.stderr, dt

def run_wxec_mcp(calls: list[dict]) -> tuple[str, float]:
    proc = subprocess.Popen(
        [str(W_BIN)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        cwd=ROOT,
        bufsize=1,
    )
    # Handshake
    proc.stdin.write(json.dumps({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "probe", "version": "1.0"}}
    }) + "\n")
    proc.stdin.flush()
    proc.stdout.readline()
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    proc.stdin.flush()

    t0 = time.perf_counter()
    req = {
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "batch", "arguments": {"calls": calls}}
    }
    proc.stdin.write(json.dumps(req) + "\n")
    proc.stdin.flush()
    line = proc.stdout.readline()
    dt = (time.perf_counter() - t0) * 1000
    proc.stdin.close()
    proc.wait(timeout=5)

    res = json.loads(line)
    items = res.get("result", {}).get("content", [])
    text = "\n".join(it.get("text", "") for it in items)
    return text, dt

def main():
    print("=" * 88)
    print("LIVE REPOSITORY PROBE: RAW STANDARD TOOLS vs WXEC MCP")
    print(f"Target Repository: {ROOT}")
    print("=" * 88)

    # -------------------------------------------------------------
    # PROBE 1: Read a 1,894-line file (crates/wxec-tools/src/fs.rs)
    # -------------------------------------------------------------
    raw_read, raw_read_ms = run_raw("cat crates/wxec-tools/src/fs.rs")
    wxec_read, wxec_read_ms = run_wxec_mcp([{"tool": "read", "args": {"path": "crates/wxec-tools/src/fs.rs", "lines": "1030:1060"}}])

    # -------------------------------------------------------------
    # PROBE 2: Unchanged file re-read (ledger cache)
    # -------------------------------------------------------------
    # In wxec, reading the same file again triggers unchanged stub
    proc = subprocess.Popen([str(W_BIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, cwd=ROOT, bufsize=1)
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "p", "version": "1"}}}) + "\n")
    proc.stdin.flush()
    proc.stdout.readline()
    # Read 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "read", "arguments": {"path": "crates/wxec-tools/src/fs.rs"}}}) + "\n")
    proc.stdin.flush()
    proc.stdout.readline()
    # Read 2 (Unchanged!)
    t0 = time.perf_counter()
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "read", "arguments": {"path": "crates/wxec-tools/src/fs.rs"}}}) + "\n")
    proc.stdin.flush()
    line_unchanged = proc.stdout.readline()
    unchanged_ms = (time.perf_counter() - t0) * 1000
    proc.stdin.close()
    proc.wait()
    unchanged_res = json.loads(line_unchanged)
    unchanged_text = "\n".join(it.get("text", "") for it in unchanged_res.get("result", {}).get("content", []))

    # -------------------------------------------------------------
    # PROBE 3: Running a test suite (cargo test wxec-tools invariants)
    # -------------------------------------------------------------
    raw_test, raw_test_ms = run_raw("cargo test -p wxec-tools --test invariants")
    wxec_test, wxec_test_ms = run_wxec_mcp([{"tool": "check", "args": {"kind": "test"}}])

    probes = [
        {
            "name": "1. Reading targeted function in 1,894-line file",
            "raw_desc": "cat fs.rs (full file dump)",
            "raw_text": raw_read,
            "raw_ms": raw_read_ms,
            "wxec_desc": "wxec read fs.rs 1030:1060 (range sliced)",
            "wxec_text": wxec_read,
            "wxec_ms": wxec_read_ms,
        },
        {
            "name": "2. Repeated read of unchanged file",
            "raw_desc": "cat fs.rs again (full re-dump)",
            "raw_text": raw_read,
            "raw_ms": raw_read_ms,
            "wxec_desc": "wxec read fs.rs (ledger hash cache)",
            "wxec_text": unchanged_text,
            "wxec_ms": unchanged_ms,
        },
        {
            "name": "3. Test execution verification",
            "raw_desc": "raw cargo test (compiler + runner)",
            "raw_text": raw_test,
            "raw_ms": raw_test_ms,
            "wxec_desc": "wxec check(kind='test')",
            "wxec_text": wxec_test,
            "wxec_ms": wxec_test_ms,
        },
    ]

    for p in probes:
        r_tok = count_tok(p["raw_text"])
        w_tok = count_tok(p["wxec_text"])
        r_lines = len(p["raw_text"].splitlines())
        w_lines = len(p["wxec_text"].splitlines())
        savings = (1 - w_tok / r_tok) * 100

        print(f"\nPROBE: {p['name']}")
        print("-" * 88)
        print(f"{'Metric':<24} {'Raw Standard Tool':<28} {'WXEC MCP':<24} {'Savings'}")
        print("-" * 88)
        print(f"{'Tokens (cl100k)':<24} {r_tok:<28} {w_tok:<24} -{savings:.1f}%")
        print(f"{'Output Lines':<24} {r_lines:<28} {w_lines:<24} -{(1 - w_lines/r_lines)*100:.1f}%")
        print(f"{'Output Bytes':<24} {len(p['raw_text']):<28} {len(p['wxec_text']):<24} -{(1 - len(p['wxec_text'])/len(p['raw_text']))*100:.1f}%")
        print(f"{'Latency':<24} {p['raw_ms']:>6.1f}ms                     {p['wxec_ms']:>6.1f}ms")
        print(f"\n[Raw Sample Head]:\n{p['raw_text'][:200].strip()}...\n")
        print(f"[WXEC Output]:\n{p['wxec_text'].strip()}\n")
        print("=" * 88)

if __name__ == "__main__":
    main()
