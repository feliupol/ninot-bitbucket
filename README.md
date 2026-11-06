# ninot-bitbucket

A stateful Bitbucket protocol fake with a Rust HTTP core and thin Python fixture bindings. The application uses an ordinary HTTP endpoint. Fixtures arrange typed state directly in Python.

```python
from ninot_bitbucket import Server

with Server() as server:
    server.state.add_repo("workspace", "repository").create_pull_request(
        1, title="A deterministic change"
    )
    # Configure the genuine application client with server.url.
    print(server.url)
```

## Supported behavior

- Cloud `/2.0` user, repository discovery and repository lookup
- Pull-request list, create, update and lookup with source/state/author/reviewer filtering
- Build statuses and absolute next-page URLs
- Raw file contents on the main branch (`/src/{mainbranch}/{path}`), seeded with `Repository.add_file(path, content)`; directory listings and `format` are not modeled

[Pinned operation metadata](spec/operations.json) records the upstream sources and digests.

Unknown operations return HTTP 501. This service models the listed protocol behavior, rather than an entire production installation.

## Fixture controls

- `server.state` exposes resource constructors and mutable typed entities.
- `server.state.credentials = ("scenario-user", "scenario-token")` enables Basic authentication.
- `server.state.when(method="GET", path="/resource", nth=2).fail(429, retry_after=0)` scripts a provider failure.
- `.corrupt()`, `.drop()` and `.timeout(delay=...)` affect real response bodies or sockets. Configure the genuine client timeout below the injected latency when testing timeouts.
- `.requests` records ordinary HTTP requests. Setup and inspection never enter the provider network surface.
- `state.snapshot()` and `state.restore(snapshot)` preserve the world and remaining fault schedule as JSON without binding it to an instance address.

## Development

`Cargo.lock` and `uv.lock` pin Rust and Python dependencies. Then run:

```bash
uv sync --locked
uv run --no-sync pytest -q
uv run --no-sync ruff check .
uv run --no-sync mypy python/src
cargo fmt --all -- --check
cargo clippy --workspace --lib -- -D warnings
```
