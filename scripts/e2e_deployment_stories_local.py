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
    "ASSETS_BUCKET": "slopmud-assets-test",
    "SLOPMUD_WAL_BACKUP_ENABLED": "1",
    "SLOPMUD_WAL_BACKUP_DIR": "/opt/slopmud/state/walbackup",
    "SLOPMUD_WAL_BACKUP_INTERVAL_S": "60",
    "SLOPMUD_WAL_BACKUP_S3_BUCKET": "slopmud-assets-test",
    "SLOPMUD_WAL_BACKUP_S3_PREFIX": "slopmud-az1/wal-backups",
    "SLOPMUD_WAL_BACKUP_UPLOAD_ENABLED": "1",
    "SLOPMUD_WAL_RESTORE_ENABLED": "auto",
    "SLOPMUD_WAL_RESTORE_CACHE_DIR": "/opt/slopmud/state/walrestore-cache",
    "SLOPMUD_WAL_RESTORE_MISSING_OK": "1"
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


def fake_adminctl_binary(tmp: Path, args_path: Path) -> Path:
    bin_path = tmp / "slopmud_adminctl"
    bin_path.write_text(
        "\n".join(
            [
                "#!/usr/bin/env python3",
                "import json",
                "import sys",
                "from pathlib import Path",
                f"Path({str(args_path)!r}).write_text(json.dumps(sys.argv[1:]), encoding='utf-8')",
                "out = Path(sys.argv[sys.argv.index('--out') + 1])",
                "out.parent.mkdir(parents=True, exist_ok=True)",
                "out.write_text('restored\\n', encoding='utf-8')",
                "",
            ]
        ),
        encoding="utf-8",
    )
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


def parse_github_output(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        out[key] = value
    return out


def run_ci_scope_case(
    changed_path: str,
    *,
    track: str = "dev",
    event_name: str = "push",
    force_scoped: bool = False,
) -> dict[str, str]:
    with tempfile.TemporaryDirectory(prefix="slopmud_ci_scope_") as d:
        tmp = Path(d)
        run(["git", "init", "-q"], env=os.environ.copy(), cwd=tmp)
        run(["git", "config", "user.email", "ci-scope@example.invalid"], env=os.environ.copy(), cwd=tmp)
        run(["git", "config", "user.name", "CI Scope Test"], env=os.environ.copy(), cwd=tmp)

        baseline = tmp / "README.md"
        baseline.write_text("baseline\n", encoding="utf-8")
        run(["git", "add", "."], env=os.environ.copy(), cwd=tmp)
        assert_ok(run(["git", "commit", "-q", "-m", "baseline"], env=os.environ.copy(), cwd=tmp))
        before = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp, text=True).strip()

        changed = tmp / changed_path
        changed.parent.mkdir(parents=True, exist_ok=True)
        changed.write_text("changed\n", encoding="utf-8")
        run(["git", "add", "."], env=os.environ.copy(), cwd=tmp)
        assert_ok(run(["git", "commit", "-q", "-m", "change"], env=os.environ.copy(), cwd=tmp))
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=tmp, text=True).strip()

        output = tmp / "github-output.txt"
        env = os.environ.copy()
        env.update(
            {
                "GITHUB_OUTPUT": str(output),
                "TRACK": track,
                "GITHUB_EVENT_NAME": event_name,
                "GITHUB_EVENT_BEFORE": before,
                "GITHUB_SHA": head,
            }
        )
        if force_scoped:
            env["FORCE_SCOPED_E2E"] = "1"
        proc = run(["bash", str(REPO / "scripts/cicd/ci_scope_e2e.sh")], env=env, cwd=tmp)
        assert_ok(proc)
        return parse_github_output(output)


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
    if "group: enterprise-cicd-v2-" not in workflow:
        raise AssertionError("CI concurrency group should be versioned to escape orphaned pre-fix runs")
    if "cancel-in-progress: true" not in workflow:
        raise AssertionError("CI concurrency must cancel stale runs within the active generation")
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
    if "artifact_path: ${{ steps.build_asset.outputs.artifact_path }}" not in workflow:
        raise AssertionError("CI build job must export the artifact path from the build_asset step")


def test_cicd_tiny_runner_memory_guards() -> None:
    workflow = (REPO / ".github/workflows/enterprise-cicd.yml").read_text(encoding="utf-8")
    workspace = (REPO / "Cargo.toml").read_text(encoding="utf-8")
    build_script = (REPO / "scripts/build_bookworm_release.sh").read_text(encoding="utf-8")
    build_assets = (REPO / "scripts/cicd/build_assets.sh").read_text(encoding="utf-8")
    bootstrap = (REPO / "scripts/cicd/bootstrap_runner.sh").read_text(encoding="utf-8")
    tf_vars = (REPO / "infra/terraform/single-az-raft-us-east-1/variables.tf").read_text(
        encoding="utf-8"
    )
    if 'SLOPMUD_CARGO_BUILD_JOBS: "1"' not in workflow:
        raise AssertionError("CI asset build must force single-job release builds on tiny runners")
    if "build_cmd+=(-j" not in build_script or "CARGO_BUILD_JOBS" not in build_script:
        raise AssertionError("bookworm release builder must honor the CI cargo job limit")
    if "[profile.devdeploy]" not in workspace or 'inherits = "dev"' not in workspace:
        raise AssertionError("dev artifacts need a low-optimization devdeploy Cargo profile")
    if (
        "SLOPMUD_CARGO_PROFILE" not in build_script
        or '--profile "${build_profile}"' not in build_script
    ):
        raise AssertionError("bookworm builder must support explicit Cargo profiles")
    if (
        'dev|sandbox) build_profile="devdeploy"' not in build_assets
        or '*) build_profile="release"' not in build_assets
        or 'target/${profile_target_dir}' not in build_assets
    ):
        raise AssertionError("asset builds must default dev/sandbox to devdeploy and keep stg/prd release")
    if "RUNNER_SWAPFILE_MB" not in bootstrap or "/sbin/mkswap" not in bootstrap:
        raise AssertionError("runner bootstrap must provision swap for tiny build hosts")
    if (
        "GITHUB_ACTIONS_RUNNER_CHANNEL_TIMEOUT" not in bootstrap
        or "10-channel-timeout.conf" not in bootstrap
        or "runner_channel_timeout_s" not in bootstrap
    ):
        raise AssertionError("runner bootstrap must raise GitHub worker IPC timeout on tiny hosts")
    if (
        "Repairing direct Rust tool shims" not in bootstrap
        or "cargo install just --locked" not in bootstrap
    ):
        raise AssertionError("runner bootstrap must keep Rust tool shims and just available after rebuilds")
    if 'sudo install -m 0755 "$just_path" /usr/local/bin/just' not in bootstrap:
        raise AssertionError("runner bootstrap must install just as a real binary outside Cargo cache")
    rust_tool_bin = 'tool_bin="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin"'
    if workflow.count(rust_tool_bin) < 4:
        raise AssertionError("CI Rust setup steps must repair cached-over Rust tool shims")
    if workflow.count("uses: Swatinem/rust-cache@v2") != 1:
        raise AssertionError("only the build job should restore Rust cache on the tiny runner")
    for required in (
        "Detect local Rust cache",
        "local_rust_cache_present",
        "target/debug/.fingerprint",
        "target/devdeploy/.fingerprint",
        "steps.local_rust_cache.outputs.present != '1'",
    ):
        if required not in workflow:
            raise AssertionError(f"CI must restore remote Rust cache only when local cache is absent: {required}")
    dep_line = "for c in gcc git jq make pkg-config just python3 tar; do"
    if dep_line not in workflow:
        raise AssertionError("CI runner dependency preflight must cover the full tiny-runner toolchain")
    for required in ("build-essential", "git", "jq", "pkg-config", "python3", "awscli", "ripgrep"):
        if required not in bootstrap:
            raise AssertionError(f"runner bootstrap must install {required}")
    if 'variable "gateway_root_volume_gib"' not in tf_vars or "default     = 24" not in tf_vars:
        raise AssertionError("gateway root volume must leave room for the self-hosted runner build cache")


def test_shard_build_script_uses_worktree_git_paths() -> None:
    build_rs = (REPO / "apps/shard_01/build.rs").read_text(encoding="utf-8")
    for bad_watch in ("rerun-if-changed=.git/HEAD", "rerun-if-changed=.git/index"):
        if bad_watch in build_rs:
            raise AssertionError(f"shard build.rs must not watch package-local git path {bad_watch}")
    for required in ('"--git-path"', '"symbolic-ref", "-q", "HEAD"', 'rerun_if_git_path("HEAD")'):
        if required not in build_rs:
            raise AssertionError(f"shard build.rs lost worktree-safe git metadata watch: {required}")


def test_cicd_e2e_scope_script_contract() -> None:
    shard = run_ci_scope_case("apps/shard_01/src/main.rs")
    if shard.get("run_e2e_core") != "1" or shard.get("run_e2e_ws") != "0":
        raise AssertionError(f"shard changes should run core E2E only: {shard}")

    ws = run_ci_scope_case("apps/ws_gateway/src/main.rs")
    if ws.get("run_e2e_core") != "0" or ws.get("run_e2e_ws") != "1":
        raise AssertionError(f"ws gateway changes should run ws E2E only: {ws}")

    bot_party = run_ci_scope_case("apps/bot_party/src/main.rs")
    if bot_party.get("run_e2e_core") != "1" or bot_party.get("run_e2e_ws") != "1":
        raise AssertionError(f"bot party changes should run core and ws E2Es: {bot_party}")

    mudproto = run_ci_scope_case("crates/mudproto/src/lib.rs")
    if mudproto.get("run_e2e_core") != "1" or mudproto.get("run_e2e_ws") != "1":
        raise AssertionError(f"shared protocol changes should run core and ws E2Es: {mudproto}")

    broker = run_ci_scope_case("crates/slopmud/src/main.rs")
    if broker.get("run_e2e_core") != "1" or broker.get("run_e2e_ws") != "0":
        raise AssertionError(f"broker-only crate changes should run core E2E only: {broker}")

    walbackupd = run_ci_scope_case("apps/slopmud_walbackupd/src/main.rs")
    if walbackupd.get("run_e2e_core") != "1" or walbackupd.get("run_e2e_ws") != "0":
        raise AssertionError(f"walbackupd changes should run core E2E only: {walbackupd}")

    workflow = run_ci_scope_case(".github/workflows/enterprise-cicd.yml")
    if workflow.get("run_e2e_core") != "1" or workflow.get("run_e2e_ws") != "1":
        raise AssertionError(f"workflow changes must run all E2Es: {workflow}")
    if workflow.get("scope_reason") != "infra-and-tools-change":
        raise AssertionError(f"workflow changes should explain infra/tool scope: {workflow}")

    docs = run_ci_scope_case("docs/deploy-notes.md")
    if docs.get("run_e2e_core") != "0" or docs.get("run_e2e_ws") != "0":
        raise AssertionError(f"docs-only changes should not run E2Es on dev: {docs}")

    stg = run_ci_scope_case("docs/deploy-notes.md", track="stg")
    if stg.get("run_e2e_core") != "1" or stg.get("run_e2e_ws") != "1":
        raise AssertionError(f"non-dev tracks must run all E2Es: {stg}")

    manual = run_ci_scope_case("docs/deploy-notes.md", event_name="workflow_dispatch")
    if manual.get("run_e2e_core") != "1" or manual.get("run_e2e_ws") != "1":
        raise AssertionError(f"manual dispatch should run all E2Es by default: {manual}")

    manual_scoped = run_ci_scope_case(
        "docs/deploy-notes.md",
        event_name="workflow_dispatch",
        force_scoped=True,
    )
    if manual_scoped.get("run_e2e_core") != "0" or manual_scoped.get("run_e2e_ws") != "0":
        raise AssertionError(f"forced scoped manual dispatch should honor path scope: {manual_scoped}")


def test_cicd_clean_checkout_asset_contract() -> None:
    workflow = (REPO / ".github/workflows/enterprise-cicd.yml").read_text(encoding="utf-8")
    build_assets = (REPO / "scripts/cicd/build_assets.sh").read_text(encoding="utf-8")
    shuttle = (REPO / "scripts/cicd/slopmud-shuttle-assets").read_text(encoding="utf-8")
    for env_key in (
        "BUILD_STATIC_WEB",
        "BUILD_SLOPMUD_WEB",
        "BUILD_INTERNAL_OIDC",
        "BUILD_SLOPMUD_ADMINCTL",
        "BUILD_WALBACKUPD",
    ):
        expected = f"{env_key}: ${{{{ steps.meta.outputs.deploy_env == 'dev' && '0' || '1' }}}}"
        if expected not in workflow:
            raise AssertionError(f"dev CI hot path must skip unused release binary: {env_key}")
    if "cargo check --profile devdeploy -p slopmud -p shard_01 --bins" not in workflow:
        raise AssertionError("dev validation must use the same low-memory profile as dev artifacts")
    if 'env:\n          RUSTFLAGS: "-D warnings"' in workflow:
        raise AssertionError("dev validation must not globally set RUSTFLAGS and split Cargo caches")
    if 'RUSTFLAGS="-D warnings" cargo test' not in workflow:
        raise AssertionError("stg validation must keep strict warning checks")
    if "missing optional env dir for asset bundle" not in build_assets:
        raise AssertionError("CI asset bundling must tolerate clean checkouts without ignored env/")
    if "ASSETS_ENV_FILES was set" not in build_assets:
        raise AssertionError("explicit env bundle requests must still fail when env files are absent")
    if "ASSETS_ENV_REQUIRED" not in build_assets:
        raise AssertionError("operators need a strict env bundle switch for release builds that require env/")
    if workflow.count("sudo -n /usr/local/bin/slopmud-shuttle-assets") < 7:
        raise AssertionError("CI deploy jobs must use the sudo-installed shuttle hook")
    if "cargo build -q -p shard_01 -p ws_gateway -p bot_party --bins" not in workflow:
        raise AssertionError("ws E2E must build every helper binary it launches, not just e2e_ws")
    if "dev-mud.slopmud.com" not in workflow or "deploy_public_smoke" not in workflow:
        raise AssertionError("dev deploy must include a blocking outside-in public telnet smoke")
    if "asset_ready_epoch" not in workflow or "deploy_public_smoke_after_asset_ready_s" not in workflow:
        raise AssertionError("dev deploy must report public-smoke latency excluding build time")
    if "if command -v aws >/dev/null 2>&1; then" not in workflow:
        raise AssertionError("dev artifact upload must tolerate runners without aws CLI")
    if "dev track continues with the local artifact" not in workflow:
        raise AssertionError("dev missing-aws fallback should be explicit in CI logs")
    if "aws CLI is required for ${TRACK} artifact upload" not in workflow:
        raise AssertionError("non-dev artifact upload must fail clearly when aws CLI is missing")
    if "always() &&" not in workflow or "needs.build.result == 'success'" not in workflow:
        raise AssertionError("deploy must evaluate its explicit gates even when optional E2Es are skipped")
    for job in ("e2e-core-local", "e2e-core-party", "e2e-ws"):
        if f"needs.{job}.result == 'skipped'" not in workflow:
            raise AssertionError(f"deploy must allow scoped-out optional job {job}")
    if "./scripts/cicd/slopmud-shuttle-assets --help" in workflow:
        raise AssertionError("CI must not assert the root-only deploy hook through the unprivileged checkout copy")
    if shuttle.find("-h|--help)") > shuttle.find("ERROR: must run as root"):
        raise AssertionError("shuttle helper should allow non-root --help while keeping deploy operations root-only")
    if 'shard_service_name="shard-01-sandbox"' not in shuttle or 'shard_bind="127.0.0.1:5009"' not in shuttle:
        raise AssertionError("sandbox deploy must define its own shard service when artifacts include shard_01")
    if 'shard_service_name="shard-01-dev"' not in shuttle or 'shard_bind="127.0.0.1:4941"' not in shuttle:
        raise AssertionError("dev deploy shard must not collide with prod shard port 5000")
    if 'if [[ "$shard_unit_exists" == "1" && "$rewrite_unit" != "1" ]]; then' not in shuttle:
        raise AssertionError("--rewrite-unit must use desired shard defaults, not stale unit env")
    if 'systemctl is-active --quiet "${shard_service_name}"' not in shuttle:
        raise AssertionError("CI deploy must fail when the shard service fails after restart")


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


def test_wal_restore_helper_noop_and_s3_recover_contract() -> None:
    with tempfile.TemporaryDirectory(prefix="slopmud_wal_restore_") as d:
        tmp = Path(d)
        script = REPO / "scripts/restore_wal_backup.sh"
        clean_env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith("SLOPMUD_WAL_") and not k.startswith("SHARD_RAFT_")
        }

        proc = run(["bash", str(script)], env=clean_env)
        assert_ok(proc)
        if "wal restore: disabled" not in proc.stdout:
            raise AssertionError(f"restore helper should no-op when disabled\n{proc.stdout}")

        auto_env = clean_env | {"SLOPMUD_WAL_RESTORE_ENABLED": "auto"}
        proc = run(["bash", str(script)], env=auto_env)
        assert_ok(proc)
        if "no target/source config; skipping" not in proc.stdout:
            raise AssertionError(f"restore helper should no-op without source config\n{proc.stdout}")

        args_path = tmp / "adminctl_args.json"
        target = tmp / "raft.jsonl"
        target.write_text("keep me\n", encoding="utf-8")
        restore_env = auto_env | {
            "SLOPMUD_ADMINCTL_BIN": str(fake_adminctl_binary(tmp, args_path)),
            "SHARD_RAFT_LOG": str(target),
            "SHARD_RAFT_NODE_ID": "n1",
            "SLOPMUD_WAL_BACKUP_S3_BUCKET": "backup-bucket",
            "SLOPMUD_WAL_BACKUP_S3_PREFIX": "prd/wal",
        }
        proc = run(["bash", str(script)], env=restore_env)
        assert_ok(proc)
        if args_path.exists() or target.read_text(encoding="utf-8") != "keep me\n":
            raise AssertionError("restore helper should not invoke adminctl for a non-empty target")

        target.write_text("", encoding="utf-8")
        proc = run(["bash", str(script)], env=restore_env)
        assert_ok(proc)
        args = json.loads(args_path.read_text(encoding="utf-8"))
        for required in (
            "wal-backup",
            "recover",
            "--out",
            str(target),
            "--s3",
            "s3://backup-bucket/prd/wal",
            "--node-id",
            "n1",
        ):
            if required not in args:
                raise AssertionError(f"restore helper did not pass {required!r}: {args}")
        if target.read_text(encoding="utf-8") != "restored\n":
            raise AssertionError("restore helper did not let adminctl write the recovered WAL")


def test_wal_restore_deploy_contract() -> None:
    onebox = (REPO / "scripts/deploy_shard_01.sh").read_text(encoding="utf-8")
    trio = (REPO / "scripts/deploy_shard_trio.sh").read_text(encoding="utf-8")
    split = (REPO / "scripts/deploy_split_raft_trio.sh").read_text(encoding="utf-8")
    restore_onebox = (REPO / "scripts/cicd/restore_onebox_stack.sh").read_text(encoding="utf-8")
    build_assets = (REPO / "scripts/cicd/build_assets.sh").read_text(encoding="utf-8")
    rebootstrap = (REPO / "scripts/cicd/build_rebootstrap_bundle.sh").read_text(encoding="utf-8")

    for label, text in (
        ("onebox deploy", onebox),
        ("trio deploy", trio),
        ("split deploy", split),
        ("onebox restore", restore_onebox),
    ):
        for needle in (
            "SLOPMUD_WAL_RESTORE_ENABLED",
            "SLOPMUD_WAL_RESTORE_S3_URI",
            "SLOPMUD_WAL_RESTORE_S3_BUCKET",
            "SLOPMUD_WAL_RESTORE_CACHE_DIR",
            "SLOPMUD_WAL_RESTORE_OVERWRITE",
            "SLOPMUD_WAL_RESTORE_MISSING_OK",
            "slopmud_walbackupd",
        ):
            if needle not in text:
                raise AssertionError(f"{label} lost WAL restore wiring: {needle}")
        if label != "onebox restore" and "ExecStartPre=/usr/local/bin/slopmud-wal-restore" not in text:
            raise AssertionError(f"{label} lost legacy WAL restore pre-start hook")
    if "ExecStartPre=/usr/local/bin/slopmud-wal-restore" in restore_onebox:
        raise AssertionError("artifact restore path should let shard_01 manage WAL restore through walbackupd")
    if (
        "bin/slopmud_adminctl" not in build_assets
        or "bin/slopmud_walbackupd" not in build_assets
        or "scripts/restore_wal_backup.sh" not in build_assets
    ):
        raise AssertionError("asset bundles must include adminctl, walbackupd, and the WAL restore helper")
    if "restore_wal_backup.sh" not in rebootstrap:
        raise AssertionError("rebootstrap bundle must include the WAL restore helper")


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
        for required in (
            "SLOPMUD_WAL_BACKUP_ENABLED=1",
            "SLOPMUD_WAL_BACKUP_S3_BUCKET=slopmud-assets-test",
            "SLOPMUD_WAL_RESTORE_ENABLED=auto",
            "SLOPMUD_WAL_RESTORE_MISSING_OK=1",
        ):
            if required not in text:
                raise AssertionError(f"rendered env lost WAL backup/restore setting {required!r}\n{text}")
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
    ("shard build script worktree git paths", test_shard_build_script_uses_worktree_git_paths),
    ("CI/CD E2E scope script contract", test_cicd_e2e_scope_script_contract),
    ("CI/CD clean checkout asset contract", test_cicd_clean_checkout_asset_contract),
    ("rapid split Raft live upgrade", test_rapid_split_raft_live_upgrade),
    ("Kubernetes and bare-metal restart budget contract", test_kubernetes_and_bare_metal_restart_budget_contract),
    ("current public one-box shard deploy", test_current_public_onebox_shard_deploy),
    ("WAL restore helper contract", test_wal_restore_helper_noop_and_s3_recover_contract),
    ("WAL restore deploy contract", test_wal_restore_deploy_contract),
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
