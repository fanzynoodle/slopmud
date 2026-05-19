#!/usr/bin/env python3
"""Render a split Raft deploy env from Terraform outputs.

The generated file intentionally sources the existing environment file instead
of copying secrets into a new artifact.
"""

from __future__ import annotations

import argparse
import json
import os
import shlex
import subprocess
from pathlib import Path


DEFAULT_TF_DIR = "infra/terraform/single-az-raft-us-east-1"


def terraform_recommended_env(tf_dir: Path) -> dict[str, str]:
    raw = subprocess.check_output(
        ["terraform", f"-chdir={tf_dir}", "output", "-json", "recommended_env"],
        text=True,
    )
    data = json.loads(raw)
    if not isinstance(data, dict):
        raise SystemExit("terraform recommended_env output was not an object")
    return {str(k): str(v) for k, v in data.items()}


def write_private(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text("\n".join(lines) + "\n", encoding="utf-8")
    os.chmod(tmp, 0o600)
    tmp.replace(path)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--terraform-dir", default=DEFAULT_TF_DIR)
    ap.add_argument("--base-env", required=True, help="existing env file to source")
    ap.add_argument("--out", required=True, help="generated env path")
    ap.add_argument("--env-name", default="prd-split-az1")
    ap.add_argument("--state-dir", default="/opt/slopmud/state")
    ap.add_argument(
        "--sbc-enabled",
        choices=("0", "1"),
        default="0",
        help="set SLOPMUD_SBC_ENABLED in the rendered env",
    )
    args = ap.parse_args()

    tf_dir = Path(args.terraform_dir).resolve()
    base_env = Path(args.base_env).resolve()
    out = Path(args.out).resolve()

    if not base_env.is_file():
        raise SystemExit(f"base env not found: {base_env}")

    env = terraform_recommended_env(tf_dir)
    required = [
        "HOST",
        "GATEWAY_HOST",
        "SHARD_ADDRS",
        "SHARD_NODE_HOSTS",
        "SHARD_NODE_IDS",
        "SHARD_RAFT_NODE_IDS",
        "SHARD_PORT",
        "SHARD_RAFT_PORT",
        "SLOPMUD_BIND",
    ]
    missing = [k for k in required if not env.get(k)]
    if missing:
        raise SystemExit(f"missing Terraform env keys: {', '.join(missing)}")
    optional = [
        "ASSETS_BUCKET",
    ]

    lines = [
        f"source {shlex.quote(str(base_env))}",
        f"ENV_NAME={shlex.quote(args.env_name)}",
    ]
    for key in required:
        lines.append(f"{key}={shlex.quote(env[key])}")
    for key in optional:
        if env.get(key):
            lines.append(f"{key}={shlex.quote(env[key])}")
    lines.extend(
        [
            f"SHARD_BIND=0.0.0.0:{shlex.quote(env['SHARD_PORT'])}",
            f"SHARD_RAFT_LOG={shlex.quote(args.state_dir + '/shard_01_groups_raft.jsonl')}",
            "SHARD_RAFT_ELECTION_MS=5000",
            "SHARD_RAFT_HEARTBEAT_MS=500",
            "SLOPMUD_ADMIN_BIND=127.0.0.1:4011",
            f"SLOPMUD_ACCOUNTS_PATH={shlex.quote(args.state_dir + '/accounts.json')}",
            f"SLOPMUD_BANS_PATH={shlex.quote(args.state_dir + '/bans.json')}",
            f"SLOPMUD_NEARLINE_DIR={shlex.quote(args.state_dir + '/nearline_scrollback')}",
            f"SLOPMUD_GOOGLE_OAUTH_DIR={shlex.quote(args.state_dir + '/google_oauth')}",
            f"SLOPMUD_BLOB_SPOOL_DIR={shlex.quote(args.state_dir + '/blob_spool')}",
            f"SLOPMUD_SBC_ENABLED={args.sbc_enabled}",
        ]
    )

    write_private(out, lines)
    print(out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
