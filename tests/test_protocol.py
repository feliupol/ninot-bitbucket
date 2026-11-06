"""Bitbucket resource behavior is observable through ordinary HTTP clients."""

import json

import httpx
import pytest

from ninot_bitbucket import Participant, Server, State


def test_pagination_and_pull_request_filtering() -> None:
    """Clients follow real next URLs and select reviewer/state matches."""
    with Server() as server, httpx.Client(base_url=server.url) as client:
        repo = server.state.add_repo("ws", "first")
        server.state.add_repo("ws", "second")
        repo.create_pull_request(1, participants=[Participant(account_id="reviewer")])
        repo.create_pull_request(
            2, state="MERGED", participants=[Participant(account_id="reviewer")]
        )
        repo.create_pull_request(3, author_account_id="other")
        first = client.get("repositories/ws", params={"pagelen": 1}).json()
        assert [item["slug"] for item in first["values"]] == ["first"]
        second = client.get(first["next"]).json()
        assert [item["slug"] for item in second["values"]] == ["second"]
        assert "next" not in second
        prs = client.get(
            "repositories/ws/first/pullrequests",
            params={"q": 'state="OPEN" AND reviewers.account_id="reviewer"'},
        ).json()
        assert [item["id"] for item in prs["values"]] == [1]


def test_create_update_and_status_observe_the_same_resource() -> None:
    """PR writes persist, and build statuses remain newest first."""
    with Server() as server, httpx.Client(base_url=server.url) as client:
        repo = server.state.add_repo("ws", "repo")
        created = client.post(
            "repositories/ws/repo/pullrequests",
            json={
                "title": "Feature",
                "source": {"branch": {"name": "feature"}},
                "destination": {"branch": {"name": "main"}},
            },
        )
        assert created.status_code == 201
        number = created.json()["id"]
        assert (
            client.put(
                f"repositories/ws/repo/pullrequests/{number}", json={"title": "Ready"}
            ).status_code
            == 200
        )
        assert repo.prs[0].title == "Ready"
        repo.prs[0].add_status("FAILED")
        repo.prs[0].add_status("SUCCESSFUL", key="ci")
        statuses = client.get(f"repositories/ws/repo/pullrequests/{number}/statuses").json()
        assert [item["state"] for item in statuses["values"]] == ["SUCCESSFUL", "FAILED"]


def test_authentication_rate_limit_and_missing_repository() -> None:
    """A retryable provider response preserves the world until recovery."""
    state = State()
    state.credentials = ("scenario", "token")
    state.when(path="/user").fail(429, retry_after=0)
    with Server(state) as server, httpx.Client(base_url=server.url) as client:
        assert client.get("user").status_code == 401
        client.auth = state.credentials
        assert client.get("user").headers["Retry-After"] == "0"
        assert client.get("user").json()["account_id"] == "acc"
        assert client.get("repositories/ws/missing").status_code == 404
        assert client.get("unsupported").status_code == 501


def test_json_snapshot_restores_ownership_and_remaining_faults() -> None:
    """Restored repositories can accept status mutations and serve another address."""
    state = State()
    state.add_repo("ws", "repo").create_pull_request(8)
    state.when(path="/user").fail(503)
    snapshot = json.loads(json.dumps(state.snapshot()))
    restored = State()
    restored.restore(snapshot)
    restored._repos["ws/repo"].prs[0].add_status("SUCCESSFUL")
    with Server(restored) as server:
        assert httpx.get(server.url + "/user").status_code == 503
        assert httpx.get(server.url + "/user").status_code == 200
    with pytest.raises(ValueError, match="unsupported Bitbucket snapshot"):
        restored.restore({"version": 2})


def test_file_contents_are_served_from_the_main_branch_only() -> None:
    """Raw bytes come from the main branch; other refs and paths answer Bitbucket 404s."""
    with Server() as server, httpx.Client(base_url=server.url) as client:
        repo = server.state.add_repo("ws", "repo", default_branch="main")
        repo.add_file(".gitmodules", '[submodule "lib"]\n\tpath = lib\n')
        repo.add_file("bin/blob", b"\xff\x00")
        text = client.get("repositories/ws/repo/src/main/.gitmodules")
        assert text.status_code == 200
        assert text.headers["content-type"] == "text/plain"
        assert text.text == '[submodule "lib"]\n\tpath = lib\n'
        blob = client.get("repositories/ws/repo/src/main/bin/blob")
        assert blob.headers["content-type"] == "application/octet-stream"
        assert blob.content == b"\xff\x00"
        wrong_ref = client.get("repositories/ws/repo/src/feature/.gitmodules")
        assert wrong_ref.status_code == 404
        assert wrong_ref.json()["error"]["message"] == "Commit not found"
        missing = client.get("repositories/ws/repo/src/main/README.md")
        assert missing.status_code == 404
        assert missing.json() == {
            "type": "error",
            "error": {"message": "No such file or directory: README.md"},
        }
        assert client.get("repositories/ws/missing/src/main/.gitmodules").status_code == 404


def test_file_contents_honor_credentials_and_faults() -> None:
    """Source reads pass through Basic authentication and fault rules like other routes."""
    state = State()
    state.credentials = ("scenario", "token")
    state.add_repo("ws", "repo", default_branch="main").add_file(".gitmodules", "x")
    state.when(path="/src/").fail(503)
    with Server(state) as server, httpx.Client(base_url=server.url) as client:
        assert client.get("repositories/ws/repo/src/main/.gitmodules").status_code == 401
        client.auth = state.credentials
        assert client.get("repositories/ws/repo/src/main/.gitmodules").status_code == 503
        assert client.get("repositories/ws/repo/src/main/.gitmodules").text == "x"


def test_snapshot_restores_file_contents() -> None:
    """A restored world serves the same file bytes from another server."""
    state = State()
    state.add_repo("ws", "repo", default_branch="main").add_file("/.gitmodules", b"data")
    restored = State()
    restored.restore(json.loads(json.dumps(state.snapshot())))
    with Server(restored) as server:
        response = httpx.get(server.url + "/repositories/ws/repo/src/main/.gitmodules")
        assert response.content == b"data"
