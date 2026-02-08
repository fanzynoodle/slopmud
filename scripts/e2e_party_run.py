#!/usr/bin/env python3
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path


class Client:
    def __init__(self, sock):
        self.sock = sock
        self.buf = b""

    def _read_some(self, timeout_s=0.2):
        self.sock.settimeout(timeout_s)
        try:
            return self.sock.recv(4096)
        except socket.timeout:
            return b""

    def read_until(self, needles, timeout_s=8.0):
        if isinstance(needles, (bytes, str)):
            needles = [needles]
        needles_b = []
        for n in needles:
            needles_b.append(n.encode("utf-8") if isinstance(n, str) else n)

        deadline = time.time() + timeout_s
        while time.time() < deadline:
            best = None
            for n in needles_b:
                i = self.buf.find(n)
                if i != -1:
                    if best is None or i < best[0]:
                        best = (i, n)
            if best is not None:
                i, n = best
                before = self.buf[: i + len(n)]
                self.buf = self.buf[i + len(n) :]
                return before

            chunk = self._read_some(timeout_s=0.25)
            if chunk:
                self.buf += chunk
            else:
                time.sleep(0.02)

        raise TimeoutError(
            f"timeout waiting for {needles!r}; got:\n{self.buf.decode('utf-8', 'replace')}"
        )


def send_line(sock, s):
    sock.sendall((s.strip() + "\n").encode("utf-8"))


def connect_and_create(name, is_bot=False, host="127.0.0.1", port=54100):
    # Broker startup can be a little slow (especially under cargo); retry connect.
    deadline = time.time() + 12.0
    last_err = None
    while time.time() < deadline:
        try:
            s = socket.create_connection((host, port), timeout=3.0)
            break
        except OSError as e:
            last_err = e
            time.sleep(0.1)
    else:
        raise last_err
    c = Client(s)
    c.read_until("name:")
    send_line(s, name)
    # Broker requires choosing an auth method, then an account password (create or login),
    # before automation disclosure. Avoid matching the "- password" bullet in the auth menu.
    pw = f"pw-{name}-1234"
    out = c.read_until(
        ["type: password", "set password", "password (never logged/echoed)"],
        timeout_s=12.0,
    )
    if b"type: password" in out:
        send_line(s, "password")
        out = c.read_until(
            ["set password", "password (never logged/echoed)"],
            timeout_s=12.0,
        )
    if b"set password" in out or b"password (never logged/echoed)" in out:
        send_line(s, pw)
    c.read_until("type: human | bot", timeout_s=12.0)
    send_line(s, "bot" if is_bot else "human")
    c.read_until("type: agree")
    send_line(s, "agree")
    c.read_until("code of conduct:")
    c.read_until("type: agree")
    send_line(s, "agree")
    # Finish creation flow at the broker (race/class/sex) before we reach the shard.
    c.read_until(["choose race:", "type: race list"], timeout_s=12.0)
    send_line(s, "race human")
    c.read_until(["choose class:", "type: class list"], timeout_s=12.0)
    send_line(s, "class fighter")
    c.read_until("sex:", timeout_s=12.0)
    send_line(s, "none")
    # The server may not auto-`look` on connect; sync on prompt then force a look.
    c.read_until(">", timeout_s=15.0)
    send_line(s, "look")
    c.read_until("Orientation Wing", timeout_s=15.0)
    return c


def print_tail(path: Path, lines: int = 120):
    try:
        data = path.read_text(encoding="utf-8", errors="replace").splitlines()
        tail = data[-lines:]
        print(f"--- tail {path} ---", file=sys.stderr)
        print("\n".join(tail), file=sys.stderr)
    except Exception as e:
        print(f"--- tail {path} failed: {e} ---", file=sys.stderr)


def cargo_build_quiet(env, build_log: Path, pkg: str):
    cmd = ["cargo", "build", "-q", "-p", pkg]
    with open(build_log, "ab") as f:
        f.write(f"$ {' '.join(cmd)}\n".encode("utf-8"))
        p = subprocess.run(cmd, env=env, stdout=f, stderr=f)
    if p.returncode != 0:
        print(f"build failed; log: {build_log}", file=sys.stderr)
        print_tail(build_log)
        raise subprocess.CalledProcessError(p.returncode, cmd)


def main():
    shard_bind = "127.0.0.1:55021"
    broker_bind = "127.0.0.1:54100"

    run_id = str(time.time_ns())

    env = os.environ.copy()
    env["SHARD_BIND"] = shard_bind
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"
    env["RUST_BACKTRACE"] = env.get("RUST_BACKTRACE", "1")
    # Keep accounts isolated per run so we always exercise the "set password" path.
    env["SLOPMUD_ACCOUNTS_PATH"] = f"/tmp/slopmud_accounts_e2e_party_{run_id}.json"

    build_log = Path(f"/tmp/slopmud_e2e_party_build_{run_id}.log")
    shard_log = Path(f"/tmp/slopmud_e2e_party_shard_{run_id}.log")
    broker_log = Path(f"/tmp/slopmud_e2e_party_broker_{run_id}.log")
    shard_f = open(shard_log, "wb")
    broker_f = open(broker_log, "wb")

    cargo_build_quiet(env, build_log, "shard_01")
    cargo_build_quiet(env, build_log, "slopmud")

    shard = subprocess.Popen(
        ["target/debug/shard_01"],
        env=env,
        stdout=shard_f,
        stderr=shard_f,
        start_new_session=True,
    )
    try:
        time.sleep(0.8)
        broker = subprocess.Popen(
            ["target/debug/slopmud"],
            env=env,
            stdout=broker_f,
            stderr=broker_f,
            start_new_session=True,
        )
    except Exception:
        os.killpg(shard.pid, signal.SIGTERM)
        raise

    ok = False
    try:
        time.sleep(0.8)
        a = connect_and_create("Alice", is_bot=False, port=54100)
        b = connect_and_create("Bob", is_bot=True, port=54100)

        # Character setup (required for movement).
        send_line(a.sock, "wear tunic")
        a.read_until("you equip training tunic", timeout_s=3.0)
        send_line(a.sock, "wield sword")
        a.read_until("you equip practice sword", timeout_s=3.0)
        send_line(b.sock, "wear tunic")
        b.read_until("you equip training tunic", timeout_s=3.0)
        send_line(b.sock, "wield sword")
        b.read_until("you equip practice sword", timeout_s=3.0)

        send_line(a.sock, "party create")
        a.read_until("party: created", timeout_s=3.0)
        send_line(a.sock, "party invite bob")
        a.read_until("party: invited", timeout_s=3.0)
        b.read_until("party invite from", timeout_s=3.0)
        send_line(b.sock, "party accept")
        b.read_until("party: joined", timeout_s=3.0)

        send_line(b.sock, "follow on")
        b.read_until("follow: on", timeout_s=3.0)

        send_line(a.sock, "party run q1-first-day-on-gaia")
        a.read_until("constructing q1-first-day-on-gaia", timeout_s=3.0)
        a.read_until("your party enters a new run", timeout_s=6.0)
        b.read_until("your party enters a new run", timeout_s=6.0)

        # Start room should be the first Room Flow entry.
        a.read_until("R_NS_ORIENT_01", timeout_s=3.0)
        b.read_until("R_NS_ORIENT_01", timeout_s=3.0)

        # Party chat.
        send_line(b.sock, "party say ready")
        a.read_until("[party", timeout_s=3.0)
        a.read_until("Bob: ready", timeout_s=3.0)

        # Leader moves east; follower should arrive.
        send_line(a.sock, "east")
        a.read_until("badge desk", timeout_s=3.0)
        b.read_until("badge desk", timeout_s=3.0)

        send_line(a.sock, "exit")
        send_line(b.sock, "exit")
        a.sock.close()
        b.sock.close()
        ok = True
        print("party e2e ok")
        return 0
    except Exception:
        print(
            f"party e2e failed; logs: {broker_log} {shard_log} {build_log}",
            file=sys.stderr,
        )
        for path in [broker_log, shard_log]:
            print_tail(path)
        raise
    finally:
        for p in [locals().get("broker"), shard]:
            if p is None:
                continue
            try:
                os.killpg(p.pid, signal.SIGTERM)
            except Exception:
                pass
        for p in [locals().get("broker"), shard]:
            if p is None:
                continue
            try:
                p.wait(timeout=3)
            except Exception:
                pass
        try:
            shard_f.close()
        except Exception:
            pass
        try:
            broker_f.close()
        except Exception:
            pass
        if ok:
            try:
                broker_log.unlink()
            except Exception:
                pass
            try:
                shard_log.unlink()
            except Exception:
                pass
            try:
                build_log.unlink()
            except Exception:
                pass


if __name__ == "__main__":
    sys.exit(main())
