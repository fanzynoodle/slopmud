#!/usr/bin/env python3
"""Machine-readable deployment/promotion DAG and invariants.

The local e2e deployment-story harness imports this module as unit coverage for
the documented workflow. It intentionally has no third-party dependencies.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


REPO = Path(__file__).resolve().parents[1]


@dataclass(frozen=True)
class Node:
    id: str
    label: str
    kind: str
    owner: str


@dataclass(frozen=True)
class Edge:
    src: str
    dst: str
    reason: str


@dataclass(frozen=True)
class Workflow:
    id: str
    start: str
    terminal: str
    required_nodes: tuple[str, ...]
    exact_artifact: bool = False
    quorum_guarded: bool = False


NODES: tuple[Node, ...] = (
    Node("code_change", "code change", "source", "developer"),
    Node("local_validation", "local validation", "validation", "developer"),
    Node("commit", "commit", "source", "developer"),
    Node("push_dev", "push dev branch", "source", "developer"),
    Node("push_stg", "push stg branch", "source", "developer"),
    Node("build_local_asset", "build local CI-style artifact", "artifact", "developer"),
    Node("build_dev_asset", "build dev release artifact", "artifact", "ci"),
    Node("publish_dev_s3", "publish dev artifact to S3", "artifact", "ci"),
    Node("build_stg_asset", "build stg release artifact", "artifact", "ci"),
    Node("publish_stg_s3", "publish stg artifact to S3", "artifact", "ci"),
    Node("sandbox_deploy", "deploy sandbox", "deploy", "ci"),
    Node("sandbox_smoke", "smoke sandbox", "smoke", "ci"),
    Node("dev_deploy", "deploy dev", "deploy", "ci"),
    Node("dev_smoke", "smoke dev", "smoke", "ci"),
    Node("stg_deploy", "deploy stg", "deploy", "ci"),
    Node("stg_smoke", "smoke stg", "smoke", "ci"),
    Node("prod_copy", "copy exact stg artifact to prod track", "artifact", "ci"),
    Node("prod_deploy", "deploy prd", "deploy", "ci"),
    Node("prod_smoke", "smoke prd", "smoke", "ci"),
    Node("onebox_stage", "stage one-box shard binary", "deploy", "operator"),
    Node("onebox_restart", "restart one-box shard", "deploy", "operator"),
    Node("onebox_smoke", "smoke one-box", "smoke", "operator"),
    Node("split_stage_direct", "stage split Raft binary directly", "deploy", "operator"),
    Node("split_stage_s3", "publish split Raft binary to S3", "artifact", "operator"),
    Node("split_prefetch_all", "prefetch on all Raft voters", "deploy", "operator"),
    Node("k8s_stage_image", "stage Kubernetes shard image", "artifact", "operator"),
    Node("leader_visible", "visible Raft leader", "quorum", "raft"),
    Node("transfer_leader", "transfer leader away from target", "quorum", "raft"),
    Node("restart_lease", "acquire Raft restart lease", "quorum", "raft"),
    Node("quorum_guard", "verify two remaining voters", "quorum", "raft"),
    Node("rolling_restart_one", "restart one voter", "deploy", "operator"),
    Node("cluster_ready", "wait cluster ready", "quorum", "raft"),
    Node("release_lease", "release restart lease", "quorum", "raft"),
    Node("raft_smoke", "smoke Raft version/status", "smoke", "operator"),
    Node("terraform_apply", "terraform apply/rebuild", "infra", "operator"),
    Node("reconcile_raft_dns", "reconcile Raft DNS", "infra", "operator"),
    Node("render_env", "render split env", "infra", "operator"),
    Node("ensure_volumes", "ensure data volumes", "infra", "operator"),
    Node("deploy_split_units", "deploy split Raft units", "deploy", "operator"),
    Node("deploy_gateway", "deploy gateway/web endpoints", "deploy", "operator"),
)


EDGES: tuple[Edge, ...] = (
    Edge("code_change", "local_validation", "validate before any release"),
    Edge("local_validation", "commit", "commit only after local validation"),
    Edge("commit", "push_dev", "dev promotion starts from committed code"),
    Edge("commit", "push_stg", "stg promotion starts from committed code"),
    Edge("commit", "build_local_asset", "local artifact deploy builds the same asset shape as CI"),
    Edge("build_local_asset", "split_stage_direct", "local deploy reuses the split Raft artifact wrapper"),
    Edge("push_dev", "build_dev_asset", "CI builds dev artifact"),
    Edge("build_dev_asset", "publish_dev_s3", "dev artifact is published once"),
    Edge("push_stg", "build_stg_asset", "CI builds stg artifact"),
    Edge("build_stg_asset", "publish_stg_s3", "stg artifact is published once"),
    Edge("publish_dev_s3", "sandbox_deploy", "dev must pass sandbox first"),
    Edge("sandbox_deploy", "sandbox_smoke", "sandbox deploy must be verified"),
    Edge("sandbox_smoke", "dev_deploy", "sandbox gates dev deploy"),
    Edge("dev_deploy", "split_stage_direct", "dev CI deploys the exact artifact to the Raft trio"),
    Edge("publish_stg_s3", "stg_deploy", "stg deploys built artifact"),
    Edge("stg_deploy", "stg_smoke", "stg deploy must be verified"),
    Edge("stg_smoke", "prod_copy", "prod promotion gated by stg smoke"),
    Edge("prod_copy", "prod_deploy", "prod deploy uses copied artifact"),
    Edge("prod_deploy", "prod_smoke", "prod deploy must be verified"),
    Edge("commit", "onebox_stage", "operator deploy starts from committed code"),
    Edge("onebox_stage", "onebox_restart", "stage before restart"),
    Edge("onebox_restart", "onebox_smoke", "restart must be smoked"),
    Edge("commit", "split_stage_direct", "direct split deploy starts from committed code"),
    Edge("commit", "split_stage_s3", "S3 split deploy starts from committed code"),
    Edge("commit", "k8s_stage_image", "Kubernetes deploy starts from committed code"),
    Edge("split_stage_s3", "split_prefetch_all", "all voters fetch immutable bytes"),
    Edge("split_stage_direct", "leader_visible", "direct staging then quorum guard"),
    Edge("split_prefetch_all", "leader_visible", "prefetch completes before activation"),
    Edge("k8s_stage_image", "leader_visible", "image staging then app-aware quorum guard"),
    Edge("leader_visible", "transfer_leader", "avoid restarting active leader"),
    Edge("transfer_leader", "restart_lease", "lease is taken after target is safe"),
    Edge("restart_lease", "quorum_guard", "lease holders still prove quorum"),
    Edge("quorum_guard", "rolling_restart_one", "only one voter restarts"),
    Edge("rolling_restart_one", "cluster_ready", "wait before next voter"),
    Edge("cluster_ready", "release_lease", "release after node rejoins"),
    Edge("release_lease", "raft_smoke", "final smoke after rollout"),
    Edge("release_lease", "deploy_gateway", "gateway is repointed after guarded voter rollout"),
    Edge("deploy_gateway", "dev_smoke", "dev gateway must be externally smoked"),
    Edge("terraform_apply", "reconcile_raft_dns", "ASG membership refreshes stable Raft DNS"),
    Edge("reconcile_raft_dns", "render_env", "runtime env uses stable DNS names"),
    Edge("render_env", "ensure_volumes", "mount persistent state before services"),
    Edge("ensure_volumes", "deploy_split_units", "state ready before Raft units"),
    Edge("deploy_split_units", "deploy_gateway", "gateway points at fresh voters"),
    Edge("deploy_gateway", "raft_smoke", "replacement story ends with smoke"),
)


WORKFLOWS: tuple[Workflow, ...] = (
    Workflow(
        "local_split_direct",
        "commit",
        "raft_smoke",
        (
            "split_stage_direct",
            "leader_visible",
            "transfer_leader",
            "restart_lease",
            "quorum_guard",
            "rolling_restart_one",
            "cluster_ready",
            "release_lease",
            "raft_smoke",
        ),
        quorum_guarded=True,
    ),
    Workflow(
        "local_split_asset",
        "commit",
        "raft_smoke",
        (
            "build_local_asset",
            "split_stage_direct",
            "leader_visible",
            "transfer_leader",
            "restart_lease",
            "quorum_guard",
            "rolling_restart_one",
            "cluster_ready",
            "release_lease",
            "deploy_gateway",
            "raft_smoke",
        ),
        exact_artifact=True,
        quorum_guarded=True,
    ),
    Workflow(
        "local_split_s3",
        "commit",
        "raft_smoke",
        (
            "split_stage_s3",
            "split_prefetch_all",
            "leader_visible",
            "transfer_leader",
            "restart_lease",
            "quorum_guard",
            "rolling_restart_one",
            "cluster_ready",
            "release_lease",
            "raft_smoke",
        ),
        exact_artifact=True,
        quorum_guarded=True,
    ),
    Workflow(
        "k8s_statefulset",
        "commit",
        "raft_smoke",
        (
            "k8s_stage_image",
            "leader_visible",
            "transfer_leader",
            "restart_lease",
            "quorum_guard",
            "rolling_restart_one",
            "cluster_ready",
            "release_lease",
            "raft_smoke",
        ),
        exact_artifact=True,
        quorum_guarded=True,
    ),
    Workflow(
        "onebox_current_public",
        "commit",
        "onebox_smoke",
        ("onebox_stage", "onebox_restart", "onebox_smoke"),
    ),
    Workflow(
        "cicd_dev",
        "push_dev",
        "dev_smoke",
        (
            "build_dev_asset",
            "publish_dev_s3",
            "sandbox_deploy",
            "sandbox_smoke",
            "dev_deploy",
            "split_stage_direct",
            "leader_visible",
            "transfer_leader",
            "restart_lease",
            "quorum_guard",
            "rolling_restart_one",
            "cluster_ready",
            "release_lease",
            "deploy_gateway",
            "dev_smoke",
        ),
        exact_artifact=True,
        quorum_guarded=True,
    ),
    Workflow(
        "cicd_stg_to_prod",
        "push_stg",
        "prod_smoke",
        (
            "build_stg_asset",
            "publish_stg_s3",
            "stg_deploy",
            "stg_smoke",
            "prod_copy",
            "prod_deploy",
            "prod_smoke",
        ),
        exact_artifact=True,
    ),
    Workflow(
        "instance_replacement",
        "terraform_apply",
        "raft_smoke",
        ("reconcile_raft_dns", "render_env", "ensure_volumes", "deploy_split_units", "deploy_gateway", "raft_smoke"),
    ),
)


def node_ids() -> set[str]:
    return {n.id for n in NODES}


def edge_pairs() -> set[tuple[str, str]]:
    return {(e.src, e.dst) for e in EDGES}


def reachable(start: str, terminal: str) -> bool:
    edges = edge_pairs()
    seen = {start}
    frontier = [start]
    while frontier:
        current = frontier.pop()
        if current == terminal:
            return True
        for src, dst in edges:
            if src == current and dst not in seen:
                seen.add(dst)
                frontier.append(dst)
    return False


def predecessor_chain(target: str) -> set[str]:
    edges = edge_pairs()
    seen: set[str] = set()
    frontier = [target]
    while frontier:
        current = frontier.pop()
        for src, dst in edges:
            if dst == current and src not in seen:
                seen.add(src)
                frontier.append(src)
    return seen


def assert_dag_invariants(repo: Path = REPO) -> None:
    ids = node_ids()
    if len(ids) != len(NODES):
        raise AssertionError("duplicate deployment DAG node id")

    for edge in EDGES:
        if edge.src not in ids:
            raise AssertionError(f"edge uses unknown src node: {edge}")
        if edge.dst not in ids:
            raise AssertionError(f"edge uses unknown dst node: {edge}")

    for workflow in WORKFLOWS:
        if workflow.start not in ids:
            raise AssertionError(f"workflow {workflow.id} has unknown start")
        if workflow.terminal not in ids:
            raise AssertionError(f"workflow {workflow.id} has unknown terminal")
        if not reachable(workflow.start, workflow.terminal):
            raise AssertionError(f"workflow {workflow.id} has no path to terminal")
        for node in workflow.required_nodes:
            if node not in ids:
                raise AssertionError(f"workflow {workflow.id} requires unknown node {node}")
            if (
                node not in (workflow.start, workflow.terminal)
                and node not in predecessor_chain(workflow.terminal)
            ):
                raise AssertionError(f"workflow {workflow.id} required node {node} is not on route")

    quorum_order = [
        "leader_visible",
        "transfer_leader",
        "restart_lease",
        "quorum_guard",
        "rolling_restart_one",
        "cluster_ready",
        "release_lease",
    ]
    positions = {node: i for i, node in enumerate(quorum_order)}
    for left, right in zip(quorum_order, quorum_order[1:]):
        if (left, right) not in edge_pairs():
            raise AssertionError(f"missing quorum ordering edge {left}->{right}")
        if positions[left] >= positions[right]:
            raise AssertionError(f"bad quorum ordering {left}->{right}")

    prod_preds = predecessor_chain("prod_deploy")
    for required in (
        "push_stg",
        "build_stg_asset",
        "publish_stg_s3",
        "stg_deploy",
        "stg_smoke",
        "prod_copy",
    ):
        if required not in prod_preds:
            raise AssertionError(f"prod deploy is not gated by {required}")

    if "ensure_volumes" not in predecessor_chain("deploy_split_units"):
        raise AssertionError("instance replacement deploys Raft before data volumes")

    required_files = [
        "scripts/deploy_split_raft_trio.sh",
        "scripts/k8s_raft_fast_restart.sh",
        "scripts/deploy_shard_01.sh",
        "scripts/cicd/deploy_slopmud_from_s3.sh",
        "scripts/cicd/deploy_split_raft_trio_from_asset.sh",
        "scripts/cicd/reconcile_raft_dns.sh",
        "scripts/render_single_az_raft_env.py",
        "scripts/ensure_data_volume_mounts.sh",
        ".github/workflows/enterprise-cicd.yml",
        "docs/deployment_stories.md",
    ]
    for rel in required_files:
        if not (repo / rel).is_file():
            raise AssertionError(f"DAG references missing file: {rel}")


def mermaid() -> str:
    labels = {n.id: n.label for n in NODES}
    lines = ["flowchart TD"]
    for edge in EDGES:
        lines.append(f"  {edge.src}[{labels[edge.src]}] --> {edge.dst}[{labels[edge.dst]}]")
    return "\n".join(lines)


def main() -> int:
    assert_dag_invariants()
    print("deployment DAG invariants ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
