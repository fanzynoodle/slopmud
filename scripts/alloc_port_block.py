#!/usr/bin/env python3
"""
Allocate an unused "port block" (a base port + offsets) to avoid collisions between
local stacks and tests.

Example:
  python3 scripts/alloc_port_block.py --range 4950-5990 --stride 5 --offsets 0,1,2,4

Behavior:
- Tries candidate base ports in ascending order (step = stride).
- For each candidate, ensures all (base + offset) ports are currently bindable.
- Uses a small lock + state file in /tmp to reduce collisions between concurrent
  allocators (best-effort; still not a hard reservation).
"""

from __future__ import annotations

import argparse
import errno
import json
import os
import socket
import sys
import time
from dataclasses import dataclass
from pathlib import Path


DEFAULT_STATE_PATH = Path(os.environ.get("SLOPMUD_PORT_ALLOC_STATE", "/tmp/slopmud_port_alloc.json"))
DEFAULT_TTL_S = int(os.environ.get("SLOPMUD_PORT_ALLOC_TTL_S", str(6 * 60 * 60)))


@dataclass(frozen=True)
class Alloc:
    base: int
    offsets: list[int]
    ts: float
    pid: int


def _parse_range(s: str) -> tuple[int, int]:
    try:
        a, b = s.split("-", 1)
        lo = int(a)
        hi = int(b)
    except Exception as e:
        raise argparse.ArgumentTypeError(f"invalid --range {s!r} (expected N-N): {e}")
    if lo <= 0 or hi <= 0 or hi < lo:
        raise argparse.ArgumentTypeError(f"invalid --range {s!r}")
    return lo, hi


def _parse_offsets(s: str) -> list[int]:
    try:
        offs = [int(x.strip()) for x in s.split(",") if x.strip() != ""]
    except Exception as e:
        raise argparse.ArgumentTypeError(f"invalid --offsets {s!r}: {e}")
    if not offs:
        raise argparse.ArgumentTypeError("--offsets must be non-empty")
    if any(o < 0 for o in offs):
        raise argparse.ArgumentTypeError("--offsets cannot contain negatives")
    # Stabilize ordering for state comparisons.
    return sorted(set(offs))


def _load_allocs(path: Path) -> list[Alloc]:
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        return []
    except Exception:
        # Corrupt state should not brick development; reset.
        return []
    allocs = []
    for a in raw.get("allocs", []):
        try:
            allocs.append(
                Alloc(
                    base=int(a["base"]),
                    offsets=[int(x) for x in a["offsets"]],
                    ts=float(a["ts"]),
                    pid=int(a.get("pid", 0)),
                )
            )
        except Exception:
            continue
    return allocs


def _save_allocs(path: Path, allocs: list[Alloc]) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    data = {
        "allocs": [
            {"base": a.base, "offsets": list(a.offsets), "ts": a.ts, "pid": a.pid} for a in allocs
        ]
    }
    tmp.write_text(json.dumps(data, sort_keys=True), encoding="utf-8")
    tmp.replace(path)


def _prune_allocs(allocs: list[Alloc], ttl_s: int) -> list[Alloc]:
    now = time.time()
    keep = []
    for a in allocs:
        if now - a.ts <= ttl_s:
            keep.append(a)
    return keep


def _reserved_ports(allocs: list[Alloc]) -> set[int]:
    out: set[int] = set()
    for a in allocs:
        for o in a.offsets:
            out.add(a.base + o)
    return out


def _ports_bindable(host: str, ports: list[int]) -> bool:
    socks: list[socket.socket] = []
    try:
        for p in ports:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            # Best-effort quick check.
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            s.bind((host, p))
            socks.append(s)
        return True
    except OSError as e:
        if e.errno == errno.EPERM:
            raise
        return False
    finally:
        for s in socks:
            try:
                s.close()
            except Exception:
                pass


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--range", dest="port_range", required=True, type=_parse_range)
    ap.add_argument("--stride", required=True, type=int)
    ap.add_argument("--offsets", required=True, type=_parse_offsets)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--state", type=Path, default=DEFAULT_STATE_PATH)
    ap.add_argument("--ttl-seconds", type=int, default=DEFAULT_TTL_S)
    args = ap.parse_args()

    if args.stride <= 0:
        ap.error("--stride must be > 0")
    if args.ttl_seconds <= 0:
        ap.error("--ttl-seconds must be > 0")

    start, end = args.port_range
    max_off = max(args.offsets)
    if start + max_off > end:
        ap.error("--range is too small for the given --offsets")

    # Lock with a sibling file to keep state writes consistent.
    lock_path = Path(str(args.state) + ".lock")
    lock_path.parent.mkdir(parents=True, exist_ok=True)
    args.state.parent.mkdir(parents=True, exist_ok=True)

    try:
        import fcntl  # linux/unix only
    except Exception:
        fcntl = None

    with open(lock_path, "a+", encoding="utf-8") as lockf:
        if fcntl is not None:
            fcntl.flock(lockf.fileno(), fcntl.LOCK_EX)

        allocs = _prune_allocs(_load_allocs(args.state), args.ttl_seconds)
        reserved = _reserved_ports(allocs)

        for base in range(start, end + 1, args.stride):
            if base + max_off > end:
                break
            ports = [base + o for o in args.offsets]
            if any(p in reserved for p in ports):
                continue
            try:
                ok = _ports_bindable(args.host, ports)
            except OSError as e:
                # If the environment blocks sockets entirely, fail explicitly so callers can decide
                # whether to skip e2e.
                if e.errno == errno.EPERM:
                    print("error: cannot probe ports (socket syscall blocked: EPERM)", file=sys.stderr)
                    return 2
                raise
            if not ok:
                continue

            allocs.append(Alloc(base=base, offsets=args.offsets, ts=time.time(), pid=os.getpid()))
            _save_allocs(args.state, allocs)
            print(base)
            return 0

    print("error: no free port block found", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
