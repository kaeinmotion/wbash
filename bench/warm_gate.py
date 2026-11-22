#!/usr/bin/env python3
import os, subprocess, sys, tempfile
from pathlib import Path

def main():
    files = int(os.environ.get("WXEC_PERF_FILES", "10000"))
    cmd = [
        "cargo",
        "test",
        "--release",
        "-q",
        "-p",
        "wxec-tools",
        "--test",
        "warm_perf",
        "--",
        "--ignored",
        "--nocapture",
    ]
    env = os.environ.copy()
    env["WXEC_PERF_FILES"] = str(files)
    env.setdefault("WXEC_WARM_READ_P95_MS", "5.0")
    env.setdefault("WXEC_WARM_SEARCH_P95_MS", "25.0")
    p = subprocess.run(cmd, env=env)
    return p.returncode

if __name__ == "__main__":
    raise SystemExit(main())
