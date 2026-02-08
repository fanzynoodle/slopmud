#!/usr/bin/env python3
import argparse
import socket
import time


def main() -> int:
    ap = argparse.ArgumentParser(description="Connect and assert slopmud accepts a session.")
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, required=True)
    ap.add_argument("--timeout-s", type=float, default=6.0)
    args = ap.parse_args()

    deadline = time.time() + args.timeout_s
    buf = b""

    s = socket.create_connection((args.host, args.port), timeout=min(3.0, args.timeout_s))
    try:
        s.settimeout(0.5)
        while time.time() < deadline:
            try:
                chunk = s.recv(4096)
            except socket.timeout:
                chunk = b""
            if chunk:
                buf += chunk
                if b"name:" in buf:
                    return 0
            else:
                time.sleep(0.05)
    finally:
        try:
            s.close()
        except Exception:
            pass

    msg = buf.decode("utf-8", "replace")
    raise SystemExit(f"smoke failed: did not see 'name:'; got:\n{msg}")


if __name__ == "__main__":
    raise SystemExit(main())
