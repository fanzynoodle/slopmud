#!/usr/bin/env python3
import argparse
import subprocess
import sys
import time


def run(cmd: list[str]) -> None:
    p = subprocess.run(cmd)
    if p.returncode != 0:
        raise SystemExit(p.returncode)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--minutes", type=float, default=10.0)
    ap.add_argument("--sleep-s", type=float, default=0.1)
    args = ap.parse_args()

    deadline = time.time() + args.minutes * 60.0
    n = 0
    while time.time() < deadline:
        n += 1
        print(f"burn: run {n}", flush=True)
        run(["python3", "scripts/e2e_local.py"])
        run(["python3", "scripts/e2e_party_run.py"])
        time.sleep(args.sleep_s)

    print(f"burn: ok ({n} runs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
