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


def connect_and_create(name, is_bot=False, host="127.0.0.1", port=54010):
    s = socket.create_connection((host, port), timeout=3.0)
    c = Client(s)
    c.read_until("name:")
    send_line(s, name)
    # Broker now requires an account password (create or login) before disclosure.
    pw = f"pw-{name}-1234"
    while True:
        out = c.read_until(["type: human | bot", "set password", "password"], timeout_s=12.0)
        if b"type: human | bot" in out:
            break
        # Either first-time password creation ("set password") or login ("password").
        send_line(s, pw)
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


def wait_for_room_occupant(sock, who, tries=40):
    for _ in range(tries):
        send_line(sock.sock, "look")
        out = sock.read_until(["here:", "huh?"], timeout_s=2.0)
        if who.encode("utf-8") in out:
            return
        time.sleep(0.15)
    raise RuntimeError(f"did not see {who} in room")


def _parse_room_name(out: bytes) -> str | None:
    # Expected header: "== Name (Area) ==\r\n"
    try:
        s = out.decode("utf-8", "replace")
    except Exception:
        return None
    i = s.find("== ")
    if i == -1:
        return None
    j = s.find(" ==", i + 3)
    if j == -1:
        return None
    return s[i + 3 : j].strip() or None


def _parse_exits(out: bytes) -> list[str]:
    try:
        s = out.decode("utf-8", "replace")
    except Exception:
        return []
    for line in s.splitlines():
        if not line.lower().startswith("exits:"):
            continue
        rest = line.split(":", 1)[1].strip()
        if not rest or rest.lower() == "none":
            return []
        exits = []
        for part in rest.split(","):
            tok = part.strip().split(" ", 1)[0].strip()
            if tok:
                exits.append(tok)
        return exits
    return []


def roam_until_worm(c: Client, max_moves=60):
    # Walk around the newbie area until we find the worm spawn room.
    tried_by_room: dict[str, set[str]] = {}
    for _ in range(max_moves):
        send_line(c.sock, "look")
        out = c.read_until("here:", timeout_s=3.0)
        room = _parse_room_name(out) or "<unknown>"
        if b"stenchworm (mob)" in out:
            return

        # The worm spawns on a timer; if we're in the right room, it may take a few seconds.
        if room not in tried_by_room:
            deadline = time.time() + 6.5
            while time.time() < deadline:
                time.sleep(0.5)
                send_line(c.sock, "look")
                out2 = c.read_until("here:", timeout_s=3.0)
                if b"stenchworm (mob)" in out2 or b"stenchworm emerges" in out2:
                    return

        exits = _parse_exits(out)
        tried = tried_by_room.setdefault(room, set())
        nxt = None
        for e in exits:
            if e not in tried:
                nxt = e
                break
        if nxt is None:
            # No untried exits from here; attempt a backtrack.
            send_line(c.sock, "back")
            c.read_until("exits:", timeout_s=5.0)
            continue
        tried.add(nxt)
        send_line(c.sock, nxt)
        c.read_until("exits:", timeout_s=5.0)


def main():
    # Dedicated ports so this is safe to run while you have a local dev session up.
    shard_bind = "127.0.0.1:55011"
    broker_bind = "127.0.0.1:54010"

    run_id = str(time.time_ns())

    env = os.environ.copy()
    env["SHARD_BIND"] = shard_bind
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_ADDR"] = shard_bind
    env["WORLD_TICK_MS"] = "200"
    env["BARTENDER_EMOTE_MS"] = "1000"
    env["RUST_BACKTRACE"] = env.get("RUST_BACKTRACE", "1")
    # Keep accounts isolated per run so we always exercise the "set password" path.
    env["SLOPMUD_ACCOUNTS_PATH"] = f"/tmp/slopmud_accounts_e2e_local_{run_id}.json"
    shard_log = Path(f"/tmp/slopmud_e2e_local_shard_{run_id}.log")
    broker_log = Path(f"/tmp/slopmud_e2e_local_broker_{run_id}.log")
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

        a = connect_and_create("Alice", is_bot=False)
        b = connect_and_create("Bob", is_bot=True)
        send_line(b.sock, "assist off")
        b.read_until("assist: off", timeout_s=3.0)

        # See each other + talk.
        wait_for_room_occupant(a, "Bob")
        wait_for_room_occupant(b, "Alice")
        send_line(a.sock, "say hi bob")
        b.read_until("Alice: hi bob", timeout_s=3.0)

        # Character setup (required for movement/combat).
        send_line(a.sock, "wear tunic")
        a.read_until("you equip training tunic", timeout_s=3.0)
        send_line(a.sock, "wear boots")
        a.read_until("you equip training boots", timeout_s=3.0)
        send_line(a.sock, "wield sword")
        a.read_until("you equip practice sword", timeout_s=3.0)
        send_line(a.sock, "equip buckler")
        a.read_until("you equip wooden buckler", timeout_s=3.0)
        send_line(b.sock, "wear tunic")
        b.read_until("you equip training tunic", timeout_s=3.0)
        send_line(b.sock, "wield sword")
        b.read_until("you equip practice sword", timeout_s=3.0)

        def walk(client, steps):
            for step in steps:
                send_line(client.sock, step)
                client.read_until("exits:", timeout_s=5.0)

        path_to_worm = ["east", "north", "west", "south", "east", "north", "east", "east"]
        path_back_to_orient = ["west", "west", "south", "west", "north", "east", "south", "west"]

        # Walk to the worm room (R_NS_LABS_03) and kill 2 worms (loot 2 pouches).
        walk(a, path_to_worm)
        for i in range(2):
            for _ in range(80):
                send_line(a.sock, "look")
                out = a.read_until("here:", timeout_s=2.0)
                if b"stenchworm (mob)" in out:
                    break
                time.sleep(0.15)
            else:
                raise RuntimeError("stenchworm never spawned")
            send_line(a.sock, "kill stenchworm")
            a.read_until("you attack.", timeout_s=3.0)
            a.read_until("stenchworm dies.", timeout_s=25.0)
            send_line(a.sock, "i")
            a.read_until(f"stenchpouch x{i+1}", timeout_s=6.0)

        # Train a skill in the orientation wing (trainer room).
        walk(a, path_back_to_orient)
        a.read_until("Orientation Wing", timeout_s=3.0)
        send_line(a.sock, "train power_strike")
        a.read_until("trainer: trained power_strike (rank 1).", timeout_s=3.0)

        # Use the skill in combat.
        walk(a, path_to_worm)
        for _ in range(80):
            send_line(a.sock, "look")
            out = a.read_until("here:", timeout_s=2.0)
            if b"stenchworm (mob)" in out:
                break
            time.sleep(0.15)
        send_line(a.sock, "kill stenchworm")
        a.read_until("you attack.", timeout_s=3.0)
        send_line(a.sock, "use power_strike")
        a.read_until("uses power_strike", timeout_s=6.0)
        a.read_until("stenchworm dies.", timeout_s=25.0)

        # Party: invite + split XP in the same room.
        send_line(a.sock, "party create")
        a.read_until("party: created", timeout_s=3.0)
        send_line(a.sock, "party invite Bob")
        a.read_until("party: invited", timeout_s=3.0)
        b.read_until("party invite from", timeout_s=3.0)
        send_line(b.sock, "party accept")
        b.read_until("party: joined", timeout_s=3.0)

        # Bring Bob into the worm room too.
        walk(b, path_to_worm)
        for _ in range(80):
            send_line(a.sock, "look")
            out = a.read_until("here:", timeout_s=2.0)
            if b"stenchworm (mob)" in out:
                break
            time.sleep(0.15)
        send_line(a.sock, "kill stenchworm")
        a.read_until("you attack.", timeout_s=3.0)
        a.read_until("party xp:", timeout_s=10.0)
        b.read_until("party xp:", timeout_s=10.0)

        send_line(a.sock, "party disband")
        a.read_until("party: disbanded", timeout_s=3.0)
        b.read_until("party: disbanded by", timeout_s=3.0)

        # Clean shutdown.
        send_line(a.sock, "exit")
        send_line(b.sock, "exit")
        a.sock.close()
        b.sock.close()

        ok = True
        print("e2e ok")
        return 0
    except Exception:
        print(f"e2e failed; logs: {broker_log} {shard_log}", file=sys.stderr)
        for path in [broker_log, shard_log]:
            try:
                data = path.read_text(encoding="utf-8", errors="replace").splitlines()
                tail = data[-120:]
                print(f"--- tail {path} ---", file=sys.stderr)
                print("\n".join(tail), file=sys.stderr)
            except Exception as e:
                print(f"--- tail {path} failed: {e} ---", file=sys.stderr)
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


if __name__ == "__main__":
    sys.exit(main())
