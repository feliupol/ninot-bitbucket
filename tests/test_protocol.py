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


def test_projects_paginate_filter_and_resolve_by_key() -> None:
    """Workspace projects page with real next URLs, match `q` terms and 404 when unknown."""
    with Server() as server, httpx.Client(base_url=server.url) as client:
        server.state.add_project("ws", "ALPHA", "Alpha Platform", description="core")
        server.state.add_project("ws", "BETA", "Beta Tools", is_private=False)
        server.state.add_project("ws", "GAMMA", "Gamma Platform")
        server.state.add_project("other", "ALPHA", "Foreign Alpha")
        first = client.get("workspaces/ws/projects", params={"pagelen": 2}).json()
        assert [item["key"] for item in first["values"]] == ["ALPHA", "BETA"]
        assert first["size"] == 3
        second = client.get(first["next"]).json()
        assert [item["key"] for item in second["values"]] == ["GAMMA"]
        assert "next" not in second
        by_name = client.get("workspaces/ws/projects", params={"q": 'name~"platform"'}).json()
        assert [item["key"] for item in by_name["values"]] == ["ALPHA", "GAMMA"]
        by_key = client.get("workspaces/ws/projects", params={"q": 'key="BETA"'}).json()
        assert [item["name"] for item in by_key["values"]] == ["Beta Tools"]
        project = client.get("workspaces/ws/projects/ALPHA").json()
        assert project["type"] == "project"
        assert project["name"] == "Alpha Platform"
        assert project["description"] == "core"
        assert project["is_private"] is True
        assert project["uuid"].startswith("{")
        assert project["uuid"].endswith("}")
        assert project["links"]["html"]["href"].endswith("/ws/workspace/projects/ALPHA")
        assert client.get("workspaces/ws/projects/ALPHA").json()["uuid"] == project["uuid"]
        missing = client.get("workspaces/ws/projects/NOPE")
        assert missing.status_code == 404
        assert missing.json()["type"] == "error"
        assert client.get("workspaces/empty/projects").json()["values"] == []


def test_repositories_carry_their_project_and_filter_by_project_key() -> None:
    """Repository responses embed the project, and `q=project.key` selects its repositories."""
    with Server() as server, httpx.Client(base_url=server.url) as client:
        project = server.state.add_project("ws", "ALPHA", "Alpha Platform")
        server.state.add_project("ws", "BETA", "Beta Tools")
        first = server.state.add_repo("ws", "first", project="ALPHA")
        server.state.add_repo("ws", "second", project="BETA", is_private=True)
        server.state.add_repo("ws", "third", project="ALPHA")
        server.state.add_repo("ws", "loose")
        assert first.project is not None
        assert first.project.key == "ALPHA"
        assert [repo.full_name for repo in project.repos] == ["ws/first", "ws/third"]
        embedded = client.get("repositories/ws/first").json()["project"]
        assert embedded["type"] == "project"
        assert embedded["key"] == "ALPHA"
        assert embedded["name"] == "Alpha Platform"
        assert embedded["uuid"] == project.uuid
        assert "links" in embedded
        assert "project" not in client.get("repositories/ws/loose").json()
        listed = client.get("repositories/ws", params={"q": 'project.key="ALPHA"'}).json()
        assert [item["slug"] for item in listed["values"]] == ["first", "third"]
        page = client.get(
            "repositories/ws", params={"q": 'project.key="ALPHA"', "pagelen": 1}
        ).json()
        assert [item["slug"] for item in page["values"]] == ["first"]
        assert [item["slug"] for item in client.get(page["next"]).json()["values"]] == ["third"]
        none = client.get("repositories/ws", params={"q": 'project.key="NOPE"'}).json()
        assert none["values"] == []
        assert client.get("workspaces/ws/projects/ALPHA").json()["has_publicly_visible_repos"]
        assert not client.get("workspaces/ws/projects/BETA").json()["has_publicly_visible_repos"]


def test_repository_project_must_exist_in_its_workspace() -> None:
    """Assigning a repository to an unregistered project fails at fixture time."""
    state = State()
    state.add_project("other", "ALPHA", "Foreign")
    with pytest.raises(LookupError, match="no project"):
        state.add_repo("ws", "repo", project="ALPHA")


def test_snapshot_restores_projects_and_assignments() -> None:
    """A restored world serves the same projects and repository assignments."""
    state = State()
    state.add_project("ws", "ALPHA", "Alpha Platform", description="core")
    state.add_repo("ws", "repo", project="ALPHA")
    restored = State()
    restored.restore(json.loads(json.dumps(state.snapshot())))
    assert restored._projects["ws/ALPHA"].name == "Alpha Platform"
    assert [repo.full_name for repo in restored._projects["ws/ALPHA"].repos] == ["ws/repo"]
    with Server(restored) as server:
        body = httpx.get(server.url + "/repositories/ws/repo").json()
        assert body["project"]["key"] == "ALPHA"
