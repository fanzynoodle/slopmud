#!/usr/bin/env python3
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path
import re


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


def connect_and_create(name, is_bot=False, host="127.0.0.1", port=54012):
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
    c.read_until(["choose race:", "type: race list"], timeout_s=12.0)
    send_line(s, "race human")
    c.read_until(["choose class:", "type: class list"], timeout_s=12.0)
    send_line(s, "class fighter")
    c.read_until("sex:", timeout_s=12.0)
    send_line(s, "none")
    c.read_until(">", timeout_s=15.0)
    return c


def start_stack(env, shard_log, broker_log):
    shard_f = open(shard_log, "wb")
    broker_f = open(broker_log, "wb")

    subprocess.check_call(
        ["cargo", "build", "-q", "-p", "shard_01"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    subprocess.check_call(
        ["cargo", "build", "-q", "-p", "slopmud"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    shard = subprocess.Popen(
        ["cargo", "run", "-q", "-p", "shard_01"],
        env=env,
        stdout=shard_f,
        stderr=shard_f,
    )
    broker = subprocess.Popen(
        ["cargo", "run", "-q", "-p", "slopmud"],
        env=env,
        stdout=broker_f,
        stderr=broker_f,
    )
    return shard, broker


def stop_proc(p):
    if p is None:
        return
    try:
        p.send_signal(signal.SIGTERM)
    except Exception:
        pass
    try:
        p.wait(timeout=4.0)
    except Exception:
        try:
            p.kill()
        except Exception:
            pass


def main():
    shard_bind = "127.0.0.1:55013"
    broker_bind = "127.0.0.1:54012"
    run_id = str(time.time_ns())

    env = os.environ.copy()
    env["SHARD_BIND"] = shard_bind
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"
    env["BARTENDER_EMOTE_MS"] = "1000"
    env["RUST_BACKTRACE"] = env.get("RUST_BACKTRACE", "1")
    env["SLOPMUD_ACCOUNTS_PATH"] = f"/tmp/slopmud_accounts_e2e_groups_{run_id}.json"
    raft_path = f"/tmp/slopmud_shard_raft_groups_{run_id}.jsonl"
    env["SHARD_RAFT_LOG"] = raft_path
    env["SHARD_BOOTSTRAP_ADMINS"] = "Alice"

    shard_log = Path(f"/tmp/slopmud_e2e_groups_shard_{run_id}.log")
    broker_log = Path(f"/tmp/slopmud_e2e_groups_broker_{run_id}.log")

    shard, broker = None, None
    try:
        shard, broker = start_stack(env, shard_log, broker_log)

        alice = connect_and_create("Alice", is_bot=False, port=54012)
        bob = connect_and_create("Bob", is_bot=False, port=54012)

        # Alice is a bootstrap admin.
        send_line(alice.sock, "raft watch on")
        alice.read_until("raft: watch on", timeout_s=8.0)

        send_line(alice.sock, "group create guild testguild")
        out = alice.read_until("group: created", timeout_s=10.0)
        out += alice._read_some(timeout_s=0.5)
        txt = out.decode("utf-8", "replace")
        # We should see the `raft[...] {"entry":{"t":"GroupCreate","group_id":...}}` line first.
        gid = None
        key = '"group_id":'
        j = txt.lower().find(key)
        if j != -1:
            k = j + len(key)
            digits = ""
            while k < len(txt) and txt[k].isdigit():
                digits += txt[k]
                k += 1
            if digits:
                gid = int(digits)
        if gid is None:
            raise RuntimeError(f"could not parse group id from:\n{txt}")

        send_line(alice.sock, f"group add {gid} Bob member")
        alice.read_until("group:", timeout_s=8.0)

        send_line(bob.sock, f"group add {gid} Alice member")
        bob.read_until("nope:", timeout_s=8.0)

        send_line(alice.sock, f"group policy set {gid} motd welcome")
        alice.read_until("policy set", timeout_s=8.0)

        # Restart shard to verify raft replay restores group state.
        stop_proc(shard)
        shard = subprocess.Popen(
            ["cargo", "run", "-q", "-p", "shard_01"],
            env=env,
            stdout=open(shard_log, "ab"),
            stderr=open(shard_log, "ab"),
        )

        # Reconnect Alice and validate group persisted.
        alice2 = connect_and_create("Alice", is_bot=False, port=54012)
        # Broker may take a moment to reconnect to the restarted shard.
        for _ in range(60):
            send_line(alice2.sock, "look")
            out = alice2.read_until(["==", "shard offline"], timeout_s=3.0)
            if b"shard offline" not in out.lower():
                break
            time.sleep(0.1)
        send_line(alice2.sock, f"group show {gid}")
        show = alice2.read_until("role_caps:", timeout_s=10.0)
        if b"motd=welcome" not in show.lower():
            raise RuntimeError("expected motd policy after replay")
        if b"bob: member" not in show.lower():
            raise RuntimeError("expected Bob membership after replay")

        # Raft file exists and has entries.
        rp = Path(raft_path)
        if not rp.exists() or rp.stat().st_size < 10:
            raise RuntimeError("raft log not written")

        print("ok: e2e_groups_raft")
        return 0
    finally:
        stop_proc(broker)
        stop_proc(shard)


if __name__ == "__main__":
    raise SystemExit(main())
