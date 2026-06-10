#!/usr/bin/env python3
"""CI build artifact → /var/www/yummi-agent 수동 배포 (SSH deploy-vm 실패 시)."""
from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import zipfile
from urllib.parse import urlparse
from urllib.request import Request, urlopen

REPO = "Waitence7/YummiLCU"
WORKFLOW = "build-yummi-agent.yml"
LCU_DEPLOY = "/home/ubuntu/Yummi/YummiLcu/deploy/agent-version.json"
WWW = "/var/www/yummi-agent"


def _token() -> str:
    for line in open(os.path.expanduser("~/.git-credentials")):
        u = urlparse(line.strip())
        if u.hostname == "github.com" and u.password:
            return u.password
    raise SystemExit("GitHub token 없음 (~/.git-credentials)")


def _api(token: str, url: str) -> dict:
    req = Request(
        url,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/vnd.github+json",
            "User-Agent": "yummi-manual-deploy",
        },
    )
    with urlopen(req) as r:
        return json.load(r)


def _download_artifact(token: str, url: str, dest: str) -> None:
    """curl -L: GitHub 인증 후 Azure SAS 리다이렉트 (Authorization 미전달)."""
    subprocess.run(
        [
            "curl",
            "-fsSL",
            "-H",
            f"Authorization: Bearer {token}",
            "-H",
            "Accept: application/vnd.github+json",
            "-o",
            dest,
            url,
        ],
        check=True,
    )


def _latest_build_run_id(token: str) -> int:
    if os.environ.get("AGENT_RUN_ID"):
        return int(os.environ["AGENT_RUN_ID"])
    runs = _api(
        token,
        f"https://api.github.com/repos/{REPO}/actions/workflows/{WORKFLOW}/runs?per_page=15",
    ).get("workflow_runs", [])
    for run in runs:
        rid = int(run["id"])
        jobs = _api(
            token, f"https://api.github.com/repos/{REPO}/actions/runs/{rid}/jobs"
        ).get("jobs", [])
        for job in jobs:
            if job.get("name") == "build" and job.get("conclusion") == "success":
                return rid
    raise SystemExit("build 성공 run 없음")


def main() -> int:
    token = _token()
    run_id = _latest_build_run_id(token)
    print(f"run {run_id}")
    arts = {
        a["name"]: a["archive_download_url"]
        for a in _api(
            token,
            f"https://api.github.com/repos/{REPO}/actions/runs/{run_id}/artifacts",
        )["artifacts"]
    }
    if not arts:
        print(f"artifact 없음 run={run_id}", file=sys.stderr)
        return 1

    tmpdir = tempfile.mkdtemp(prefix="yummi-agent-dl.")
    print(f"download → {tmpdir}")
    for name, download_url in arts.items():
        zpath = os.path.join(tmpdir, f"{name}.zip")
        _download_artifact(token, download_url, zpath)
        with zipfile.ZipFile(zpath) as zf:
            zf.extractall(os.path.join(tmpdir, name))
        print(f"  ok {name}")

    manifest = os.path.join(tmpdir, "agent-version-json", "agent-version.json")
    with open(manifest, encoding="utf-8") as f:
        ver = json.load(f)["version"]

    portable = os.path.join(tmpdir, "YummiAgent-win-x64-portable")
    setup_dir = os.path.join(tmpdir, "YummiAgent-Setup")
    copies = [
        (os.path.join(portable, "YummiAgent-win-x64-portable.zip"), f"{WWW}/YummiAgent.zip"),
        (os.path.join(portable, "YummiAgent-patch.zip"), f"{WWW}/YummiAgent-patch.zip"),
        (
            os.path.join(setup_dir, f"YummiAgent-Setup-{ver}.exe"),
            f"{WWW}/YummiAgent-Setup-{ver}.exe",
        ),
        (manifest, f"{WWW}/agent-version.json"),
    ]
    subprocess.run(["sudo", "mkdir", "-p", WWW], check=True)
    for src, dst in copies:
        if not os.path.isfile(src):
            print(f"MISSING {src}", file=sys.stderr)
            return 1
        subprocess.run(["sudo", "cp", src, dst], check=True)
        subprocess.run(["sudo", "chmod", "644", dst], check=True)
        print(f"  → {dst}")

    subprocess.run(["cp", manifest, LCU_DEPLOY], check=True)
    print(f"배포 완료 v{ver}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
