# Session lifecycle: create, list, dashboard

Status: ready-for-agent

## What to build

Create the `shush-bin` binary crate and implement the server scaffold with session create/list lifecycle, from Rust through REST API to browser dashboard.

**Binary scaffold:**
- `shush-bin/Cargo.toml` with all dependencies (tokio, axum, clap, tower-http, tracing, reqwest, base64)
- `main.rs` — clap dispatch: `shush server` and `shush client` subcommands
- `cli.rs` — CLI definition: `ServerCmd` (no sub-flags yet), `ClientCmd` with `Submit`, `Sessions`, `Stream`, `Yolo` subcommands

**Server skeleton:**
- Axum router with `/api` prefix, CORS layer (allow localhost origins), static file serving for frontend
- Shared state: `Arc<SessionManager>`

**Session manager:**
- `session_manager.rs` — `SessionManager` struct backed by `Arc<RwLock<HashMap<Uuid, Session>>>`
- `create(name, host) -> Result<Uuid>` — create session struct, spawn tmux session + Control Mode client
- `delete(id)` — kill tmux session, remove from store
- `get(id) -> Session`, `list() -> Vec<Session>`

**tmux_control:**
- `tmux_control.rs` — `TmuxControlModeClient` struct
- `spawn(session_name: &str) -> Result<Self>` — runs `tmux -L shush new-session -d -s <name>` then `tmux -L shush -CC attach -t <name>`
- Manages child process stdin/stdout with Tokio
- `event_stream() -> impl Stream<Item=TmuxEvent>` — async stream of parsed tmux events
- `send_keys(text: &str)` — writes to stdin (Control Mode command)
- `active_pane: Option<String>` — tracked from window-pane-changed events

**REST endpoints:**
- `POST /api/sessions` — create session
- `GET /api/sessions` — list all sessions
- `GET /api/sessions/:id` — session details
- `DELETE /api/sessions/:id` — delete session

**Frontend scaffold + dashboard:**
- Vite + TypeScript project setup (`package.json`, `tsconfig.json`, `vite.config.ts`, `index.html`)
- `frontend/src/main.ts` — entry point with routing (placeholder for monitor route)
- `frontend/src/api.ts` — `createSession`, `listSessions`, `getSession`, `deleteSession` (REST client)
- `frontend/src/types.ts` — TypeScript interfaces matching API schema
- `frontend/src/dashboard/dashboard.ts` — session list page with create form, session cards, delete buttons
- `frontend/src/dashboard/dashboard.css` — dark theme styling

## Acceptance criteria

- [x] `cargo build` compiles both crates without errors
- [x] `cargo run -- server &` starts and listens on `127.0.0.1:8100`
- [x] `curl -X POST http://127.0.0.1:8100/api/sessions -H 'Content-Type: application/json' -d '{"name":"test","host":""}'` returns a session with an id
- [x] `curl http://127.0.0.1:8100/api/sessions` returns the created session
- [x] `cd frontend && npm install && npm run build` succeeds
- [x] Browser at `http://127.0.0.1:8100` shows dashboard with session list and create form
- [x] Creating a session via the browser form shows it in the list
- [x] Deleting a session via the browser removes it from the list

## Blocked by

- Slice 01: Core crate types, tmux parser, markers (`.scratch/shush-v01/issues/01-core-crate-types-tmux-parser-markers.md`)

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 1, Phase 3 session_manager + tmux_control, Phase 4 sessions routes, Phase 5 dashboard)
