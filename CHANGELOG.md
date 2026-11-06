# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versions follow the calendar: `<year>.<ISO week>.<n>`.

## [2026.45.0] - 2026-11-05

### Added

- Stateful, in-memory fake of the Bitbucket Cloud REST API for end-to-end tests that must not touch the network or a real account.
- User, repository discovery and repository lookup under `/2.0`.
- Pull requests: list, create, update and lookup, filtered by source, state, author and reviewer.
- Build statuses, with absolute next-page URLs.
- Raw file contents on the main branch, seeded with `Repository.add_file`.
- Optional Basic authentication.
- Latency and fault injection: error statuses, corrupted or dropped responses and timeouts, scheduled per request.
- Recording of the requests a client makes.
- JSON snapshots of the whole world, restorable on another server.
- Operations without an implementation answer `501 Not Implemented`.
- Python bindings that arrange typed state directly from test fixtures.
- Requires Rust 1.88 or newer (edition 2024).
