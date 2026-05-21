#!/usr/bin/env python3
"""Local tests for deploy, promotion, and replacement stories.

The unit tests validate the machine-readable deployment DAG. The integration
tests run the real deploy wrapper scripts against fake ssh/scp/aws/terraform
commands, so quorum and artifact-promotion control flow stays covered without
touching the network.
"""

from __future__ import annotations

import json
import os
import stat
import subprocess
import sys
import tempfile
from pathlib import Path

import deployment_story_dag as dag


REPO = Path(__file__).resolve().parents[1]


FAKE_SSH = r"""#!/usr/bin/env python3
import base64
import json
import os
import re
import sys
from pathlib import Path


state_path = Path(os.environ["FAKE_STATE"])


def load():
    with state_path.open("r", encoding="utf-8") as f:
        return json.load(f)


def save(state):
    tmp = state_path.with_name(f".{state_path.name}.{os.getpid()}.tmp")
    with tmp.open("w", encoding="utf-8") as f:
        json.dump(state, f, sort_keys=True)
    tmp.replace(state_path)


def log(state, kind, **fields):
    event_path = state_path.with_suffix(".events.jsonl")
    with event_path.open("a", encoding="utf-8") as f:
        f.write(json.dumps({"kind": kind, **fields}, sort_keys=True) + "\n")


def parse_args(argv):
    opts = []
    target = None
    command = ""
    i = 0
    while i < len(argv):
        a = argv[i]
        if target is None and a in ("-o", "-p", "-l", "-i", "-F", "-J"):
            opts.extend(argv[i : i + 2])
            i += 2
            continue
        if target is None and a.startswith("-"):
            opts.append(a)
            i += 1
            continue
        if target is None:
            target = a
            command = " ".join(argv[i + 1 :])
            break
        i += 1
    return opts, target or "", command


def host_from_target(target):
    if "@" in target:
        return target.rsplit("@", 1)[1]
    return target


def node_for_host(state, host):
    return state.get("host_to_node", {}).get(host)


def raft_rpc(state, node, command):
    m = re.search(r"PAYLOAD_B64='([^']+)'", command)
    if not m:
        print("missing PAYLOAD_B64", file=sys.stderr)
        return 1
    payload = base64.b64decode(m.group(1)).decode("utf-8")
    req = json.loads(payload)
    t = req.get("t")
    leader = state.get("leader", "n1")
    if t == "StatusReq":
        role = "Leader" if node == leader else "Follower"
        print(json.dumps({"t": "StatusResp", "node_id": node, "role": role, "leader_id": leader}))
        return 0
    if t == "TransferLeaderReq":
        target = req.get("target_id")
        accepted = node == leader and target in state.get("host_to_node", {}).values()
        if accepted:
            state["leader"] = target
            log(state, "transfer_leader", from_node=node, to_node=target)
            save(state)
            print(json.dumps({"t": "TransferLeaderResp", "accepted": True, "leader_id": target}))
        else:
            print(json.dumps({"t": "TransferLeaderResp", "accepted": False, "leader_id": leader, "reason": "node is not leader"}))
        return 0
    if t == "RestartLeaseReq":
        target = req.get("node_id")
        token = req.get("token")
        if node != leader:
            print(json.dumps({"t": "RestartLeaseResp", "accepted": False, "leader_id": leader, "node_id": target, "token": token, "reason": "node is not leader"}))
            return 0
        if target == leader:
            print(json.dumps({"t": "RestartLeaseResp", "accepted": False, "leader_id": leader, "node_id": target, "token": token, "reason": "node is still leader"}))
            return 0
        state["lease"] = {"node_id": target, "token": token}
        log(state, "lease_acquire", leader=node, node=target, token=token)
        save(state)
        print(json.dumps({"t": "RestartLeaseResp", "accepted": True, "leader_id": leader, "node_id": target, "token": token, "expires_in_ms": 60000}))
        return 0
    if t == "RestartLeaseReleaseReq":
        target = req.get("node_id")
        token = req.get("token")
        log(state, "lease_release", leader=node, node=target, token=token)
        state.pop("lease", None)
        save(state)
        print(json.dumps({"t": "RestartLeaseReleaseResp", "accepted": True, "leader_id": leader}))
        return 0
    print(json.dumps({"t": "ErrResp", "message": "unknown fake rpc"}))
    return 0


def main():
    opts, target, command = parse_args(sys.argv[1:])
    host = host_from_target(target)
    state = load()
    node = node_for_host(state, host)
    log(state, "ssh", target=target, host=host, node=node, opts=opts, command=command)
    save(state)

    if "sudo ss -Htnp" in command:
        active = state.get("active_host", "10.0.0.1")
        shard_port = state.get("shard_port", "5000")
        print(f"{active}:{shard_port}")
        return 0

    if "PAYLOAD_B64=" in command:
        return raft_rpc(state, node, command)

    if "aws s3 cp" in command and node:
        state = load()
        log(state, "remote_s3_pull", node=node, host=host, command=command)
        save(state)
        return 0

    if "systemctl restart" in command:
        state = load()
        log(state, "restart", node=node, host=host, command=command)
        leader = state.get("leader")
        if node and leader:
            for h, n in state.get("host_to_node", {}).items():
                if n == leader:
                    state["active_host"] = h
                    break
        save(state)
        return 0

    if "slopmud-shuttle-assets" in command:
        state = load()
        log(state, "shuttle_deploy", host=host, command=command)
        save(state)
        return 0

    if "ss -lnt" in command or "ss -lntp" in command:
        return 0

    if command.endswith("bash -se") or " bash -se" in command:
        state = load()
        log(state, "ensure_mount", host=host, node=node, opts=opts, command=command)
        save(state)
        sys.stdin.read()
        return 0

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
"""


FAKE_SCP = r"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

state_path = Path(os.environ["FAKE_STATE"])
with state_path.with_suffix(".events.jsonl").open("a", encoding="utf-8") as f:
    f.write(json.dumps({"kind": "scp", "argv": sys.argv[1:]}, sort_keys=True) + "\n")
"""


FAKE_AWS = r"""#!/usr/bin/env python3
import json
import os
import sys
from pathlib import Path

state_path = Path(os.environ["FAKE_STATE"])
argv = sys.argv[1:]
with state_path.with_suffix(".events.jsonl").open("a", encoding="utf-8") as f:
    f.write(json.dumps({"kind": "aws", "argv": argv}, sort_keys=True) + "\n")

if argv[:1] == ["sts"]:
    print("123456789012")
    raise SystemExit(0)
if argv[:2] == ["s3api", "head-object"]:
    raise SystemExit(0)
if argv[:2] == ["s3api", "list-objects-v2"]:
    print("prod/fallbacksha/artifact.tgz")
    raise SystemExit(0)
if argv[:2] == ["s3", "cp"]:
    raise SystemExit(0)
raise SystemExit(0)
"""


FAKE_TERRAFORM = r"""#!/usr/bin/env python3
import json

print(json.dumps({
    "HOST": "54.0.0.10",
    "GATEWAY_HOST": "54.0.0.10",
    "SHARD_ADDRS": "10.10.0.11:5000,10.10.0.12:5000,10.10.0.13:5000",
    "SHARD_NODE_HOSTS": "10.10.0.11,10.10.0.12,10.10.0.13",
    "SHARD_NODE_IDS": "n0,n1,n2",
    "SHARD_RAFT_NODE_IDS": "n0,n1,n2",
    "SHARD_PORT": "5000",
    "SHARD_RAFT_PORT": "5100",
    "SLOPMUD_BIND": "0.0.0.0:4200",
    "ASSETS_BUCKET": "slopmud-assets-test"
}))
"""


def write_executable(path: Path, text: str) -> None:
    path.write_text(text, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def make_fake_env(tmp: Path, state: dict) -> tuple[Path, dict[str, str]]:
    fake_bin = tmp / "fake-bin"
    fake_bin.mkdir()
    write_executable(fake_bin / "ssh", FAKE_SSH)
    write_executable(fake_bin / "scp", FAKE_SCP)
    write_executable(fake_bin / "aws", FAKE_AWS)
    write_executable(fake_bin / "terraform", FAKE_TERRAFORM)
    state_path = tmp / "state.json"
    state_path.write_text(json.dumps(state, sort_keys=True), encoding="utf-8")
    env = os.environ.copy()
    env["PATH"] = f"{fake_bin}{os.pathsep}{env.get('PATH', '')}"
    env["FAKE_STATE"] = str(state_path)
    return state_path, env


def read_state(path: Path) -> dict:
    state = json.loads(path.read_text(encoding="utf-8"))
    events_path = path.with_suffix(".events.jsonl")
    events = []
    if events_path.exists():
        for line in events_path.read_text(encoding="utf-8").splitlines():
            if line.strip():
                events.append(json.loads(line))
    state["events"] = events
    return state


def run(cmd: list[str], *, env: dict[str, str], cwd: Path = REPO) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )


def split_env_file(tmp: Path) -> Path:
    env_file = tmp / "split.env"
    env_file.write_text(
        "\n".join(
            [
                "ENV_NAME=test-split",
                "HOST=gateway.example",
                "GATEWAY_HOST=gateway.example",
                "SSH_USER=admin",
                "SSH_PORT=2222",
                "REMOTE_ROOT=/opt/slopmud",
                "SHARD_REMOTE_BIN=/opt/slopmud/bin/shard_01",
                "SHARD_NODE_HOSTS=10.0.0.1,10.0.0.2,10.0.0.3",
                "SHARD_NODE_IDS=n0,n1,n2",
                "SHARD_RAFT_NODE_IDS=n0,n1,n2",
                "SHARD_PORT=5000",
                "SHARD_RAFT_PORT=5100",
                "ASSETS_BUCKET=slopmud-assets-test",
                "",
            ]
        ),
        encoding="utf-8",
    )
    return env_file


def fake_shard_binary(tmp: Path) -> Path:
    bin_path = tmp / "shard_01"
    bin_path.write_text("#!/usr/bin/env sh\nexit 0\n", encoding="utf-8")
    bin_path.chmod(0o755)
    return bin_path


def base_cluster_state() -> dict:
    return {
        "leader": "n1",
        "active_host": "10.0.0.1",
        "shard_port": "5000",
        "host_to_node": {
            "10.0.0.1": "n0",
            "10.0.0.2": "n1",
            "10.0.0.3": "n2",
        },
        "events": [],
    }


def deploy_env(tmp: Path, state: dict, *, s3: bool = False) -> tuple[Path, dict[str, str]]:
    state_path, env = make_fake_env(tmp, state)
    env["SLOPMUD_SKIP_BUILD"] = "1"
    env["SLOPMUD_BIN_SRC"] = str(fake_shard_binary(tmp))
    env["SLOPMUD_RELEASE_ID"] = "testrelease"
    env["SLOPMUD_STRICT_LIVE_UPGRADE"] = "1"
    env["SLOPMUD_QUORUM_RESTART_GUARD"] = "1"
    env["SLOPMUD_RAFT_RESTART_LEASE"] = "required"
    env["SLOPMUD_ALLOW_UNGRACEFUL_LEADER_RESTART"] = "0"
    if s3:
        env["SLOPMUD_DEPLOY_FROM_S3"] = "1"
    return state_path, env


def assert_ok(proc: subprocess.CompletedProcess[str]) -> None:
    if proc.returncode != 0:
        raise AssertionError(f"command failed rc={proc.returncode}\n{proc.stdout}")


def event_kinds(events: list[dict]) -> list[str]:
    return [e["kind"] for e in events]


def workflow_by_id(workflow_id: str) -> dag.Workflow:
    for workflow in dag.WORKFLOWS:
        if workflow.id == workflow_id:
            return workflow
    raise AssertionError(f"missing workflow {workflow_id}")


def assert_workflow_contains(workflow_id: str, nodes: tuple[str, ...]) -> None:
    workflow = workflow_by_id(workflow_id)
    missing = [node for node in nodes if node not in workflow.required_nodes]
    if missing:
        raise AssertionError(f"workflow {workflow_id} is missing required nodes: {missing}")


def test_deployment_dag_invariants() -> None:
    dag.assert_dag_invariants(REPO)
    mermaid = dag.mermaid()
    if "flowchart TD" not in mermaid or "verify two remaining voters" not in mermaid:
        raise AssertionError("deployment DAG mermaid output lost important labels")


def test_deployment_dag_workflow_coverage() -> None:
    expected = {
        "local_split_direct",
        "local_split_s3",
        "onebox_current_public",
        "cicd_dev",
        "cicd_stg_to_prod",
        "k8s_statefulset",
        "instance_replacement",
    }
    actual = {workflow.id for workflow in dag.WORKFLOWS}
    if actual != expected:
        raise AssertionError(f"workflow set drifted: expected {expected}, got {actual}")
    covered_steps = {workflow.start for workflow in dag.WORKFLOWS}
    for workflow in dag.WORKFLOWS:
        covered_steps.add(workflow.terminal)
        covered_steps.update(workflow.required_nodes)
    uncovered = {
        node.id
        for node in dag.NODES
        if node.kind not in {"source", "validation"} and node.id not in covered_steps
    }
    if uncovered:
        raise AssertionError(f"DAG has operational steps outside workflow coverage: {sorted(uncovered)}")


def test_deployment_dag_quorum_contract() -> None:
    quorum_order = (
        "leader_visible",
        "transfer_leader",
        "restart_lease",
        "quorum_guard",
        "rolling_restart_one",
        "cluster_ready",
        "release_lease",
    )
    for workflow_id in ("local_split_direct", "local_split_s3", "k8s_statefulset"):
        workflow = workflow_by_id(workflow_id)
        if not workflow.quorum_guarded:
            raise AssertionError(f"{workflow_id} must be marked quorum guarded")
        assert_workflow_contains(workflow_id, quorum_order)
    direct_path = [edge.dst for edge in dag.EDGES if edge.src == "split_stage_direct"]
    s3_path = [edge.dst for edge in dag.EDGES if edge.src == "split_prefetch_all"]
    k8s_path = [edge.dst for edge in dag.EDGES if edge.src == "k8s_stage_image"]
    if direct_path != ["leader_visible"] or s3_path != ["leader_visible"] or k8s_path != ["leader_visible"]:
        raise AssertionError("split/k8s deploy paths must converge before quorum guarded activation")


def test_deployment_dag_promotion_contracts() -> None:
    for workflow_id in ("cicd_dev", "cicd_stg_to_prod"):
        workflow = workflow_by_id(workflow_id)
        if not workflow.exact_artifact:
            raise AssertionError(f"{workflow_id} must preserve exact artifact identity")
    prod_preds = dag.predecessor_chain("prod_deploy")
    for required in (
        "push_stg",
        "build_stg_asset",
        "publish_stg_s3",
        "stg_deploy",
        "stg_smoke",
        "prod_copy",
    ):
        if required not in prod_preds:
            raise AssertionError(f"prod deploy is no longer gated by {required}")
    if "push_dev" in prod_preds:
        raise AssertionError("prod promotion must not be driven by the dev branch")


def test_cicd_runner_inventory_fallback_does_not_skip_deploy() -> None:
    workflow = (REPO / ".github/workflows/enterprise-cicd.yml").read_text(encoding="utf-8")
    if "runner_count=1" not in workflow:
        raise AssertionError("CI runner inventory fallback must not make build/deploy jobs skip")
    if "allowing self-hosted scheduling to proceed" not in workflow:
        raise AssertionError("CI runner inventory fallback should explain that scheduling is deferred")
    if "fromJSON(needs.runner_check.outputs.runner_count) > 0" not in workflow:
        raise AssertionError("CI build job must remain gated on a positive runner count")
    if "runner_count=0" in workflow.partition("gh api repos/")[2].partition("exit 0")[0]:
        raise AssertionError("CI runner inventory API failure path still emits runner_count=0")


def test_cicd_asset_build_has_heartbeat() -> None:
    workflow = (REPO / ".github/workflows/enterprise-cicd.yml").read_text(encoding="utf-8")
    if "build_asset_heartbeat" not in workflow:
        raise AssertionError("CI asset build step needs a heartbeat for long quiet release builds")
    if "trap cleanup_heartbeat EXIT" not in workflow:
        raise AssertionError("CI asset build heartbeat must be cleaned up on failure")
    if "artifact_path=\"$(./scripts/cicd/build_assets.sh)\"" not in workflow:
        raise AssertionError("CI asset build should still capture the build_assets artifact path")


def test_cicd_tiny_runner_memory_guards() -> None:
    workflow = (REPO / ".github/workflows/enterprise-cicd.yml").read_text(encoding="utf-8")
    build_script = (REPO / "scripts/build_bookworm_release.sh").read_text(encoding="utf-8")
    bootstrap = (REPO / "scripts/cicd/bootstrap_runner.sh").read_text(encoding="utf-8")
    if 'SLOPMUD_CARGO_BUILD_JOBS: "1"' not in workflow:
        raise AssertionError("CI asset build must force single-job release builds on tiny runners")
    if "build_cmd+=(-j" not in build_script or "CARGO_BUILD_JOBS" not in build_script:
        raise AssertionError("bookworm release builder must honor the CI cargo job limit")
    if "RUNNER_SWAPFILE_MB" not in bootstrap or "/sbin/mkswap" not in bootstrap:
        raise AssertionError("runner bootstrap must provision swap for tiny build hosts")
    if "Repairing direct Rust tool shims" not in bootstrap or "cargo install just --locked" not in bootstrap:
        raise AssertionError("runner bootstrap must keep Rust tool shims and just available after rebuilds")


def test_cicd_clean_checkout_asset_contract() -> None:
    workflow = (REPO / ".github/workflows/enterprise-cicd.yml").read_text(encoding="utf-8")
    build_assets = (REPO / "scripts/cicd/build_assets.sh").read_text(encoding="utf-8")
    shuttle = (REPO / "scripts/cicd/slopmud-shuttle-assets").read_text(encoding="utf-8")
    for env_key in ("BUILD_STATIC_WEB", "BUILD_SLOPMUD_WEB", "BUILD_INTERNAL_OIDC"):
        expected = f"{env_key}: ${{{{ steps.meta.outputs.deploy_env == 'dev' && '0' || '1' }}}}"
        if expected not in workflow:
            raise AssertionError(f"dev CI hot path must skip unused release binary: {env_key}")
    if "missing optional env dir for asset bundle" not in build_assets:
        raise AssertionError("CI asset bundling must tolerate clean checkouts without ignored env/")
    if "ASSETS_ENV_FILES was set" not in build_assets:
        raise AssertionError("explicit env bundle requests must still fail when env files are absent")
    if "ASSETS_ENV_REQUIRED" not in build_assets:
        raise AssertionError("operators need a strict env bundle switch for release builds that require env/")
    if workflow.count("sudo -n /usr/local/bin/slopmud-shuttle-assets") < 7:
        raise AssertionError("CI deploy jobs must use the sudo-installed shuttle hook")
    if "./scripts/cicd/slopmud-shuttle-assets --help" in workflow:
        raise AssertionError("CI must not assert the root-only deploy hook through the unprivileged checkout copy")
    if shuttle.find("-h|--help)") > shuttle.find("ERROR: must run as root"):
        raise AssertionError("shuttle helper should allow non-root --help while keeping deploy operations root-only")


def test_rapid_split_raft_live_upgrade() -> None:
    with tempfile.TemporaryDirectory(prefix="slopmud_deploy_story_") as d:
        tmp = Path(d)
        state_path, env = deploy_env(tmp, base_cluster_state())
        env["SLOPMUD_FAST_ROLLING_RESTART"] = "1"
        env["SLOPMUD_ROLLING_RESTART_BUDGET_MS"] = "5000"
        env["SLOPMUD_SSH_MULTIPLEX"] = "1"
        proc = run(["bash", "scripts/deploy_split_raft_trio.sh", str(split_env_file(tmp))], env=env)
        assert_ok(proc)
        state = read_state(state_path)
        restarts = [e["node"] for e in state["events"] if e["kind"] == "restart"]
        if restarts != ["n2", "n0", "n1"]:
            raise AssertionError(f"unexpected restart order {restarts}\n{proc.stdout}")
        transfers = [e for e in state["events"] if e["kind"] == "transfer_leader"]
        if not transfers or transfers[-1]["to_node"] != "n2":
            raise AssertionError(f"leader was not transferred before final restart: {transfers}")
        for node in restarts:
            lease_i = next(i for i, e in enumerate(state["events"]) if e["kind"] == "lease_acquire" and e["node"] == node)
            restart_i = next(i for i, e in enumerate(state["events"]) if e["kind"] == "restart" and e["node"] == node)
            release_i = next(i for i, e in enumerate(state["events"]) if e["kind"] == "lease_release" and e["node"] == node)
            if not (lease_i < restart_i < release_i):
                raise AssertionError(f"restart for {node} was not bracketed by lease acquire/release")
        if proc.stdout.count("Quorum guard:") != 3:
            raise AssertionError(f"expected quorum guard before each restart\n{proc.stdout}")
        if "Bare-metal fast restart budget_ms=5000" not in proc.stdout:
            raise AssertionError(f"fast bare-metal restart budget was not announced\n{proc.stdout}")
        if "Rolling restart elapsed_ms=" not in proc.stdout:
            raise AssertionError(f"rolling restart elapsed timing was not printed\n{proc.stdout}")
        restart_commands = "\n".join(e["command"] for e in state["events"] if e["kind"] == "restart")
        if "systemctl --no-pager --full status" in restart_commands:
            raise AssertionError("fast restart path should not dump systemd status for each node")
        ssh_opts = [opt for e in state["events"] if e["kind"] == "ssh" for opt in e.get("opts", [])]
        if "ControlMaster=auto" not in ssh_opts:
            raise AssertionError(f"fast restart path should reuse SSH control sockets: {ssh_opts}")
        aws_events = [e for e in state["events"] if e["kind"] == "aws"]
        if aws_events:
            raise AssertionError(f"rapid direct deploy should not call aws: {aws_events}")


def test_kubernetes_and_bare_metal_restart_budget_contract() -> None:
    justfile = (REPO / "Justfile").read_text(encoding="utf-8")
    split_script = (REPO / "scripts/deploy_split_raft_trio.sh").read_text(encoding="utf-8")
    k8s_script = (REPO / "scripts/k8s_raft_fast_restart.sh").read_text(encoding="utf-8")
    docs = (REPO / "docs/deployment_stories.md").read_text(encoding="utf-8")
    if 'SLOPMUD_ROLLING_RESTART_BUDGET_MS="${SLOPMUD_ROLLING_RESTART_BUDGET_MS:-5000}"' not in justfile:
        raise AssertionError("bare-metal fast split restart must default to a 5 second budget")
    if 'rolling_restart_budget_ms="${SLOPMUD_ROLLING_RESTART_BUDGET_MS:-5000}"' not in split_script:
        raise AssertionError("split deploy script lost the 5 second fast-mode default")
    if 'SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS="${SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS:-10000}"' not in justfile:
        raise AssertionError("Kubernetes restart just recipe must default to 10 seconds")
    if 'budget_ms="${SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS:-10000}"' not in k8s_script:
        raise AssertionError("Kubernetes restart script lost the 10 second default")
    if 'raft_port="${SHARD_RAFT_PORT:-5001}"' not in k8s_script:
        raise AssertionError("Kubernetes restart script must default to the StatefulSet Raft RPC port 5001")
    if 'node_ids_csv="${statefulset}-0,${statefulset}-1,${statefulset}-2"' not in k8s_script:
        raise AssertionError("Kubernetes restart script must default Raft node ids to StatefulSet pod names")
    for needle in (
        "TransferLeaderReq",
        "RestartLeaseReq",
        "Kubernetes quorum guard:",
        "--grace-period=0 --force --wait=false",
        "Kubernetes rolling restart elapsed_ms=",
    ):
        if needle not in k8s_script:
            raise AssertionError(f"Kubernetes fast restart script lost app-aware guard: {needle}")
    for needle in (
        "SLOPMUD_ROLLING_RESTART_BUDGET_MS=5000",
        "SLOPMUD_K8S_ROLLING_RESTART_BUDGET_MS=10000",
    ):
        if needle not in docs:
            raise AssertionError(f"deployment docs lost restart budget contract: {needle}")


def test_current_public_onebox_shard_deploy() -> None:
    with tempfile.TemporaryDirectory(prefix="slopmud_deploy_story_") as d:
        tmp = Path(d)
        state_path, env = make_fake_env(tmp, {"events": []})
        env["SLOPMUD_SKIP_BUILD"] = "1"
        env["SLOPMUD_BIN_SRC"] = str(fake_shard_binary(tmp))
        env["SLOPMUD_RELEASE_ID"] = "oneboxrel"
        env_file = tmp / "onebox.env"
        env_file.write_text(
            "\n".join(
                [
                    "ENV_NAME=prd",
                    "HOST=public.example",
                    "SSH_USER=admin",
                    "SSH_PORT=2222",
                    "REMOTE_ROOT=/opt/slopmud",
                    "SHARD_APP_NAME=shard-01-prd",
                    "SHARD_REMOTE_BIN=/opt/slopmud/bin/shard-01-prd",
                    "SHARD_BIND=127.0.0.1:5000",
                    "",
                ]
            ),
            encoding="utf-8",
        )

        proc = run(["bash", "scripts/deploy_shard_01.sh", str(env_file)], env=env)
        assert_ok(proc)
        state = read_state(state_path)
        if "Skipping build; using " not in proc.stdout:
            raise AssertionError(f"one-box deploy should be able to reuse a built binary\n{proc.stdout}")
        if "Waiting for listen (port 5000)" not in proc.stdout:
            raise AssertionError(f"one-box deploy should keep the post-restart listen check\n{proc.stdout}")

        scp_events = [e for e in state["events"] if e["kind"] == "scp"]
        if not any(any("/tmp/shard_01.oneboxrel" in a for a in e["argv"]) for e in scp_events):
            raise AssertionError(f"one-box deploy did not upload a versioned temp binary: {scp_events}")

        ssh_commands = "\n".join(e["command"] for e in state["events"] if e["kind"] == "ssh")
        if "/opt/slopmud/bin/releases/shard_01-oneboxrel" not in ssh_commands:
            raise AssertionError("one-box deploy did not install into a versioned release path")
        if 'sudo ln -sfn "/opt/slopmud/bin/releases/shard_01-oneboxrel" "/opt/slopmud/bin/shard-01-prd.next"' not in ssh_commands:
            raise AssertionError("one-box deploy did not atomically stage the next binary symlink")
        restarts = [e for e in state["events"] if e["kind"] == "restart"]
        if not any('systemctl restart "shard-01-prd.service"' in e["command"] for e in restarts):
            raise AssertionError(f"one-box deploy did not restart the shard service: {restarts}")


def test_split_raft_s3_fanout_upgrade() -> None:
    with tempfile.TemporaryDirectory(prefix="slopmud_deploy_story_") as d:
        tmp = Path(d)
        state_path, env = deploy_env(tmp, base_cluster_state(), s3=True)
        proc = run(["bash", "scripts/deploy_split_raft_trio.sh", str(split_env_file(tmp))], env=env)
        assert_ok(proc)
        state = read_state(state_path)
        events = state["events"]
        remote_pulls = [e for e in events if e["kind"] == "remote_s3_pull"]
        if sorted(e["node"] for e in remote_pulls) != ["n0", "n1", "n2"]:
            raise AssertionError(f"expected every raft node to prefetch from S3, got {remote_pulls}")
        if not all("timeout '60' aws s3 cp" in e["command"] for e in remote_pulls):
            raise AssertionError(f"S3 prefetch must have a bounded timeout: {remote_pulls}")
        first_restart = next(i for i, e in enumerate(events) if e["kind"] == "restart")
        last_pull = max(i for i, e in enumerate(events) if e["kind"] == "remote_s3_pull")
        if not last_pull < first_restart:
            raise AssertionError("S3 prefetch must complete before process restarts")
        aws_cps = [e for e in events if e["kind"] == "aws" and e["argv"][:2] == ["s3", "cp"]]
        cp_targets = [e["argv"][2] if len(e["argv"]) > 2 else "" for e in aws_cps]
        if not any(t.endswith("/shard_01") for t in cp_targets) or not any(t.endswith("/shard_01.sha256") for t in cp_targets):
            raise AssertionError(f"missing shard binary or checksum upload: {aws_cps}")
        scp_events = [e for e in events if e["kind"] == "scp"]
        binary_scps = [e for e in scp_events if any("shard_01.testrelease" in a for a in e["argv"])]
        if binary_scps:
            raise AssertionError(f"S3 fanout should not scp the binary: {binary_scps}")


def test_cicd_s3_redeploy_wrapper() -> None:
    with tempfile.TemporaryDirectory(prefix="slopmud_deploy_story_") as d:
        tmp = Path(d)
        state_path, env = make_fake_env(tmp, {"events": []})
        env_file = tmp / "prd.env"
        env_file.write_text(
            "\n".join(
                [
                    "ENV_NAME=prd",
                    "HOST=gateway.example",
                    "SSH_USER=admin",
                    "SSH_PORT=2222",
                    "SLOPMUD_BIND=0.0.0.0:4200",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        proc = run(["bash", "scripts/cicd/deploy_slopmud_from_s3.sh", str(env_file)], env=env)
        assert_ok(proc)
        state = read_state(state_path)
        shuttle = [e for e in state["events"] if e["kind"] == "shuttle_deploy"]
        if len(shuttle) != 1:
            raise AssertionError(f"expected one shuttle deploy command, got {shuttle}")
        expected_uri = "s3://slopmud-assets-123456789012-us-east-1/prod/latest/artifact.tgz"
        if expected_uri not in shuttle[0]["command"]:
            raise AssertionError(f"CI/CD deploy did not use prod latest artifact: {shuttle[0]}")
        if "--env \"prd\"" not in shuttle[0]["command"]:
            raise AssertionError(f"CI/CD deploy did not preserve env name: {shuttle[0]}")
        if "Listening check (port 4200)" not in proc.stdout:
            raise AssertionError(f"missing post-deploy listen check\n{proc.stdout}")


def test_node_replacement_env_render_and_mount_targets() -> None:
    with tempfile.TemporaryDirectory(prefix="slopmud_deploy_story_") as d:
        tmp = Path(d)
        state_path, env = make_fake_env(tmp, {"events": []})
        base_env = tmp / "base.env"
        base_env.write_text(
            "\n".join(
                [
                    "SSH_USER=admin",
                    "SSH_PORT=2222",
                    "REMOTE_ROOT=/opt/slopmud",
                    "SHARD_REMOTE_BIN=/opt/slopmud/bin/shard_01",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        rendered = tmp / "rendered.env"
        proc = run(
            [
                "python3",
                "scripts/render_single_az_raft_env.py",
                "--terraform-dir",
                str(tmp / "tf"),
                "--base-env",
                str(base_env),
                "--out",
                str(rendered),
                "--env-name",
                "prd-split-az1",
            ],
            env=env,
        )
        assert_ok(proc)
        text = rendered.read_text(encoding="utf-8")
        if "SHARD_NODE_HOSTS=10.10.0.11,10.10.0.12,10.10.0.13" not in text:
            raise AssertionError(f"rendered env did not carry replacement node hosts\n{text}")
        if "SHARD_ADDRS=10.10.0.11:5000,10.10.0.12:5000,10.10.0.13:5000" not in text:
            raise AssertionError(f"rendered env did not carry replacement shard addrs\n{text}")
        mode = stat.S_IMODE(rendered.stat().st_mode)
        if mode != 0o600:
            raise AssertionError(f"rendered env should be private 0600, got {oct(mode)}")

        proc = run(["bash", "scripts/ensure_data_volume_mounts.sh", str(rendered), "all"], env=env)
        assert_ok(proc)
        state = read_state(state_path)
        mounts = [e for e in state["events"] if e["kind"] == "ensure_mount"]
        hosts = [e["host"] for e in mounts]
        if hosts != ["54.0.0.10", "10.10.0.11", "10.10.0.12", "10.10.0.13"]:
            raise AssertionError(f"unexpected mount targets after node replacement: {hosts}")
        raft_mounts = mounts[1:]
        if not all(any(opt == "ProxyJump=admin@54.0.0.10:2222" for opt in e["opts"]) for e in raft_mounts):
            raise AssertionError(f"raft mount checks did not use gateway ProxyJump: {raft_mounts}")


TESTS = [
    ("deployment DAG invariants", test_deployment_dag_invariants),
    ("deployment DAG workflow coverage", test_deployment_dag_workflow_coverage),
    ("deployment DAG quorum contract", test_deployment_dag_quorum_contract),
    ("deployment DAG promotion contracts", test_deployment_dag_promotion_contracts),
    ("CI/CD runner inventory fallback", test_cicd_runner_inventory_fallback_does_not_skip_deploy),
    ("CI/CD asset build heartbeat", test_cicd_asset_build_has_heartbeat),
    ("CI/CD tiny runner memory guards", test_cicd_tiny_runner_memory_guards),
    ("CI/CD clean checkout asset contract", test_cicd_clean_checkout_asset_contract),
    ("rapid split Raft live upgrade", test_rapid_split_raft_live_upgrade),
    ("Kubernetes and bare-metal restart budget contract", test_kubernetes_and_bare_metal_restart_budget_contract),
    ("current public one-box shard deploy", test_current_public_onebox_shard_deploy),
    ("split Raft S3 fanout upgrade", test_split_raft_s3_fanout_upgrade),
    ("CI/CD S3 redeploy wrapper", test_cicd_s3_redeploy_wrapper),
    ("node replacement env render and mount targets", test_node_replacement_env_render_and_mount_targets),
]


def main() -> int:
    for name, fn in TESTS:
        fn()
        print(f"[ok] {name}")
    print("deployment story local tests ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
