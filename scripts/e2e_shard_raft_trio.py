#!/usr/bin/env python3
import argparse
import os
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))

from e2e_local import Client, _pick_free_port, connect_and_create, send_line  # noqa: E402


def wait_for_port(addr: str, timeout_s: float = 12.0) -> None:
    host, port_s = addr.rsplit(":", 1)
    port = int(port_s)
    deadline = time.time() + timeout_s
    last = None
    while time.time() < deadline:
        try:
            with socket.create_connection((host, port), timeout=0.5):
                return
        except OSError as e:
            last = e
            time.sleep(0.1)
    raise RuntimeError(f"timed out waiting for {addr}: {last}")


def wait_for_file_nonempty(path: Path, timeout_s: float = 8.0) -> None:
    deadline = time.time() + timeout_s
    while time.time() < deadline:
        if path.exists() and path.stat().st_size > 0:
            return
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for nonempty {path}")


def stop_proc(proc: subprocess.Popen | None) -> None:
    if proc is None:
        return
    if proc.poll() is not None:
        return
    try:
        os.killpg(proc.pid, signal.SIGTERM)
        proc.wait(timeout=5)
    except Exception:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except Exception:
            pass


def ensure_built(env: dict[str, str], skip_build: bool) -> None:
    if skip_build and Path("target/debug/shard_01").exists() and Path("target/debug/slopmud").exists():
        return
    subprocess.check_call(
        ["cargo", "build", "-q", "-p", "shard_01", "-p", "slopmud"],
        env=env,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )


def assert_no_failover_notice(data: bytes) -> None:
    text = data.decode("utf-8", "replace").lower()
    bad = [
        "shard disconnected",
        "shard offline",
        "input dropped",
        "reconnecting",
        "hi alice",
    ]
    for needle in bad:
        if needle in text:
            raise RuntimeError(f"visible failover artifact {needle!r} in:\n{text}")


def current_leader_index(client: Client) -> int:
    send_line(client.sock, "raft status")
    out = client.read_until(
        [" - quorum_recent: true", " - quorum_recent: false"], timeout_s=8.0
    )
    out += client._read_some(timeout_s=0.1)
    text = out.decode("utf-8", "replace")
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("- node_id: n"):
            return int(line.rsplit("n", 1)[1])
    raise RuntimeError(f"could not parse leader node from raft status:\n{text}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--skip-build", action="store_true")
    args = ap.parse_args()

    try:
        _pick_free_port()
    except OSError as e:
        if getattr(e, "errno", None) in (1, 13):
            print("SKIP: e2e_shard_raft_trio requires TCP sockets", file=sys.stderr)
            return 0
        raise

    run_id = str(time.time_ns())
    shard_addrs = [f"127.0.0.1:{_pick_free_port()}" for _ in range(3)]
    raft_addrs = [f"127.0.0.1:{_pick_free_port()}" for _ in range(3)]
    broker_bind = f"127.0.0.1:{_pick_free_port()}"
    raft_paths = [
        Path(f"/tmp/slopmud_shard_raft_trio_{run_id}_{i}.jsonl") for i in range(3)
    ]

    env = os.environ.copy()
    env["RUST_BACKTRACE"] = env.get("RUST_BACKTRACE", "1")
    env["WORLD_TICK_MS"] = "100"
    env["BARTENDER_EMOTE_MS"] = "1000"
    env["MOB_WANDER_MS"] = "10000"
    env["SLOPMUD_BIND"] = broker_bind
    env["SHARD_ADDR"] = shard_addrs[0]
    env["SHARD_ADDRS"] = ",".join(shard_addrs)
    env["SLOPMUD_ACCOUNTS_PATH"] = f"/tmp/slopmud_accounts_trio_{run_id}.json"
    env["SLOPMUD_EVENTLOG_ENABLED"] = "0"
    env["SLOPMUD_NEARLINE_ENABLED"] = "0"
    env["SHARD_RAFT_HEARTBEAT_MS"] = "50"
    env["SHARD_BOOTSTRAP_ADMINS"] = "Alice"

    ensure_built(env, args.skip_build)

    log_dir = Path("/tmp") / f"slopmud_trio_{run_id}"
    log_dir.mkdir(parents=True, exist_ok=True)
    shards: list[subprocess.Popen | None] = [None, None, None]
    restart_counts = [0, 0, 0]
    broker: subprocess.Popen | None = None

    def start_shard(i: int) -> subprocess.Popen:
        shard_env = env.copy()
        shard_env["SHARD_BIND"] = shard_addrs[i]
        shard_env["NODE_ID"] = f"shard-trio-{i}"
        shard_env["SHARD_RAFT_NODE_ID"] = f"n{i}"
        shard_env["SHARD_RAFT_BIND"] = raft_addrs[i]
        shard_env["SHARD_RAFT_LOG"] = str(raft_paths[i])
        if restart_counts[i] == 0:
            election_ms = 220 + (i * 170)
        else:
            # A restarted old leader should catch up as a follower instead of
            # immediately preempting the replacement leader because it has the
            # lowest deterministic timeout.
            election_ms = 2000 + (i * 100)
        shard_env["SHARD_RAFT_ELECTION_MS"] = str(election_ms)
        shard_env["SHARD_RAFT_PEERS"] = ",".join(
            f"n{j}@{raft_addrs[j]}" for j in range(3) if j != i
        )
        log = open(log_dir / f"shard_{i}.log", "ab")
        proc = subprocess.Popen(
            ["target/debug/shard_01"],
            env=shard_env,
            stdout=log,
            stderr=log,
            start_new_session=True,
        )
        wait_for_port(shard_addrs[i])
        return proc

    def restart_shard(i: int) -> None:
        restart_counts[i] += 1
        shards[i] = start_shard(i)

    try:
        shards[0] = start_shard(0)
        shards[1] = start_shard(1)
        shards[2] = start_shard(2)
        for addr in raft_addrs:
            wait_for_port(addr)

        broker_log = open(log_dir / "broker.log", "ab")
        broker = subprocess.Popen(
            ["target/debug/slopmud"],
            env=env,
            stdout=broker_log,
            stderr=broker_log,
            start_new_session=True,
        )
        wait_for_port(broker_bind)
        broker_port = int(broker_bind.rsplit(":", 1)[1])

        alice: Client = connect_and_create("Alice", is_bot=False, port=broker_port)
        alice._read_some(timeout_s=0.3)

        wait_for_file_nonempty(raft_paths[0], timeout_s=15.0)
        send_line(alice.sock, "quest set trio.probe alive")
        out = alice.read_until("quest: set trio.probe=alive", timeout_s=8.0)
        assert_no_failover_notice(out)

        killed_leaders: list[int] = []
        for step in range(1, 4):
            victim = current_leader_index(alice)
            killed_leaders.append(victim)
            stop_proc(shards[victim])
            shards[victim] = None

            send_line(alice.sock, "quest get trio.probe")
            out = alice.read_until("quest: trio.probe=alive", timeout_s=20.0)
            out += alice._read_some(timeout_s=0.3)
            assert_no_failover_notice(out)

            send_line(alice.sock, f"quest set trio.step {step}")
            out = alice.read_until(f"quest: set trio.step={step}", timeout_s=8.0)
            assert_no_failover_notice(out)

            restart_shard(victim)

        if len(set(killed_leaders)) < 2:
            raise RuntimeError(f"expected leadership to move, killed leaders: {killed_leaders}")

        send_line(alice.sock, "quest get trio.step")
        out = alice.read_until("quest: trio.step=3", timeout_s=8.0)
        assert_no_failover_notice(out)

        print("ok: e2e_shard_raft_trio")
        return 0
    finally:
        stop_proc(broker)
        for proc in shards:
            stop_proc(proc)


if __name__ == "__main__":
    raise SystemExit(main())
