"""The operation map accounts for the complete pinned Bitbucket API description."""

import json
import re
import subprocess
import sys
from pathlib import Path

import httpx

from ninot_bitbucket import Server, State

ROOT = Path(__file__).resolve().parents[1]


def test_coverage_manifest_matches_the_full_pinned_spec() -> None:
    """Every upstream operation has a deliberate supported or 501 disposition."""
    subprocess.run(
        [sys.executable, "scripts/bitbucket_spec_coverage.py", "--check"],
        cwd=ROOT,
        check=True,
    )


def test_every_declared_operation_matches_its_coverage_disposition() -> None:
    """Supported operations route, while every unsupported spec operation returns 501."""
    manifest = json.loads((ROOT / "spec/operations.json").read_text())
    state = State()
    repo = state.add_repo("ws", "repo", default_branch="main")
    pull = repo.create_pull_request(1)
    pull.add_status("SUCCESSFUL")
    repo.add_file("README.md", "spec probe")

    substitutions = {
        "{workspace}": "ws",
        "{repo_slug}": "repo",
        "{pull_request_id}": "1",
        "{commit}": "main",
        "{path}": "README.md",
        "{selected_user}": "user",
    }
    mismatches = []
    with Server(state) as server, httpx.Client(base_url=server.url) as client:
        for operation in manifest["operations"]:
            path = operation["path"]
            for placeholder, value in substitutions.items():
                path = path.replace(placeholder, value)
            path = re.sub(r"\{[^}]+\}", "probe", path)
            path = path.removeprefix("/2.0")
            body = (
                {
                    "title": "Spec probe",
                    "source": {"branch": {"name": "feature"}},
                    "destination": {"branch": {"name": "main"}},
                }
                if operation["method"] == "POST" and operation["path"].endswith("/pullrequests")
                else None
            )
            response = client.request(operation["method"], path, json=body)
            if operation["implementation"] == "supported":
                if response.status_code == 501:
                    mismatches.append((operation["method"], operation["path"], 501))
            else:
                if response.status_code != 501:
                    mismatches.append((
                        operation["method"],
                        operation["path"],
                        response.status_code,
                    ))
    assert not mismatches, mismatches
