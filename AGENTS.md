## Rules

- DO NOT read anything in `tmp/` unless explicitly told to.
- DO NOT commit unless the user explicitly asks.
- Only implement one issue at a time.
- Before implementation, plan for TDD (use `/tdd` skill when available).
- During implementation:
  - If plans have technical gaps: stop and get user input for big gaps
  - Tick `[x]` on issue acceptance-criteria entries when done.
  - MUST write unit tests.
  - SHOULD write E2E tests.
  - MUST pass all tests before marking complete.
- After implementation, request an update to Plans and/or ADRs to close any remaining gaps.

## Must read

- `docs/shush-v01-plan.md`
- `docs/shush-v01-plan-diagrams.md`
- `docs/issues/*` for issue ownership; smaller issue number = higher priority.
- `docs/adr/*` when decisions or tradeoffs are needed.

## Repository shape

- Rust workspace in `Cargo.toml` with members:
  - `crates/shush-core` (shared types/parsers/marker logic)
  - `crates/shush-bin` (binary and Axum API server)
- `crates/shush-bin` has both CLI entry and HTTP server:
  - `crates/shush-bin/src/main.rs`
  - `crates/shush-bin/src/cli.rs` (`shush server --port <PORT>` supported)
  - `crates/shush-bin/src/server.rs` handles `/api/*` and serves `frontend/dist`.
- The `client` CLI subcommand is parsed but currently not implemented.
- Frontend app entrypoints in `frontend/src/main.ts`:
  - `/monitor/:id` -> monitor page
  - anything else -> dashboard page
- Server can serve UI directly from `frontend/dist`, but Vite dev (`npm run dev`) proxies `/api` to `127.0.0.1:8100`.

## Installation and full test

- `make install`
- `make e2e`
- Frontend E2E defaults:
  - `SHUSH_E2E_REMOTE=1`
  - `SHUSH_E2E_ASSERT_STREAM=1`
  - `SHUSH_E2E_REMOTE_HOST=shush-docker`
  - `SHUSH_E2E_CONTAINER=shush-remote-ssh`

## High-signal commands

- Rust backend:
  - `cargo test --verbose`
  - `cargo clippy --workspace --all-targets --all-features`
- Targeted verification:
  - `cargo test -p shush-core`
  - `cargo test -p shush-bin -- <test_filter>`
- Frontend:
  - `cd frontend && npm run test`
  - `cd frontend && npm run test -- <path-or-pattern>` (single Vitest target)
