# shush v0.1 Implementation Plan

## Scope & Non-Scope

### Building
- SS (shush server): Rust binary, manages tmux sessions, REST + WebSocket API
- SC (shush client): same binary via subcommand (`shush client submit ...`)
- Browser frontend: TypeScript + Vite + xterm.js
- Local + remote tmux sessions (SSH to remote hosts, direct tmux for local)
- Command lifecycle: IDLE → PENDING → EXECUTING → IDLE
- YOLO mode (per-session, skip approval)
- Marker-based command wrapping with crypto nonce
- Browser Terminal (xterm.js, read-only, broadcast via FE master connection)
- Browser Command List (command cards with approve/deny/abort)
- Abort: SIGINT → kill-pane fallback
- Command queue: in-memory FIFO, sequential execution
### Not Building (v0.1)
- Authentication / TLS (loopback only, `127.0.0.1` bind)
- Risk classification / policy engine
- Audit trail beyond server logs
- Persistence beyond in-memory (cards lost on restart)
- Federation / multi-user
- tmux pane splitting tracking (one window, one pane)

## Architecture

```
+-----------+     REST         +----------+     tmux direct      +-----------------+
|  Browser  |◄────────────────►|   SS     |◄────────────────────►|  tmux (local)   |
|  (xterm)  |   + WebSocket    |  (Rust)  |     or SSH + tmux    |  tmux (remote)  |
|  (cards)  |   (term stream)  |          |                      |                 |
+-----------+                  +----------+                      +-----------------+
                                     ▲
                                     │ REST (no WebSocket)
                                     ▼
                                +----------+
                                |   SC     |
                                |  (agent) |
                                +----------+
```

Two tmux client processes per session managed by SS:
- **SC connection**: `tmux -L shush -CC attach -t <session>` (Control Mode, PTY-backed client process)
- **FE connection**: `tmux -L shush attach -t <session> -r` (read-only, PTY-backed client process, raw ANSI stream, spawned on-demand for browser viewers)

Terminology note: `FE` in backend docs means frontend-facing tmux stream handled by backend `crates/shush-bin/src/fe_master.rs`, not TypeScript frontend app under `frontend/`.

Implementation note (2026-06-02): command completion is now driven from persistent SC control-mode `%output` stream, not FE bytes. FE remains browser-facing only and backend `fe_master.rs` strips wrapped marker echoes / APC marker sequences before forwarding terminal bytes to xterm.js.

The `host` field on Session determines the backend: `""`/`"localhost"` = direct tmux, `"user@host"` = SSH.

## Project Structure

```
shush/
├── Cargo.toml                    # workspace
├── crates/
│   ├── shush-core/               # shared types, tmux protocol parser, marker injection
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── session.rs         # Session, SessionState, CommandCard
│   │       ├── tmux_event.rs      # TmuxEvent enum, parser
│   │       └── marker.rs          # MarkerInjector, nonce generation, marker detection
│   └── shush-bin/                 # binary: `shush server` / `shush client`
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs                 # clap dispatch
│           ├── cli.rs                  # CLI definition
│           ├── server.rs               # Axum app, routes, WebSocket handler
│           ├── session_manager.rs      # in-memory Session store + card history
│           ├── command_executor.rs     # approved-command execution + marker completion
│           ├── tmux_control.rs         # SC control-mode client + registry
│           ├── fe_master.rs            # FE stream attach, filtering, snapshot/replay
│           └── remote_tmux.rs          # local/SSH tmux helpers
├── frontend/
│   ├── package.json
│   ├── tsconfig.json
│   ├── vite.config.ts
│   ├── index.html
│   └── src/
│       ├── main.ts
│       ├── api.ts                     # REST + WebSocket client
│       ├── types.ts                   # Session, CommandCard interfaces
│       ├── dashboard/
│       │   ├── dashboard.ts           # session list page
│       │   └── dashboard.css
│       └── monitor/
│           ├── monitor.ts             # monitor view: terminal + cards
│           ├── terminal.ts            # xterm.js wrapper
│           ├── card_list.ts           # command card list + approve/deny/abort
│           └── monitor.css
└── Cargo.lock
```

## API Specification

### REST Endpoints

Base URL: `http://127.0.0.1:8100/api`

Implementation note (2026-06-01): `shush server` now supports `--port <u16>`. Default remains `8100`; E2E uses a per-run ephemeral port and threads a shared runtime base URL.

| Method | Path | Request Body | Response | Description |
|--------|------|-------------|----------|-------------|
| POST | `/sessions` | `{ "name": "dev", "host": "" }` | `Session` | Create session (host empty = local) |
| GET | `/sessions` | — | `Session[]` | List all sessions |
| GET | `/sessions/:id` | — | `Session` | Session details + current command |
| DELETE | `/sessions/:id` | — | — | Kill tmux session, remove from SS |
| POST | `/sessions/:id` | `?action=submit` `{"command":"npm test"}` | `Session` | Submit command (enters PENDING, or EXECUTING if YOLO) |
| POST | `/sessions/:id` | `?action=approve` | `Session` | Approve current pending command → EXECUTING |
| POST | `/sessions/:id` | `?action=deny` | `Session` | Deny current pending command → IDLE |
| POST | `/sessions/:id` | `?action=abort` | `Session` | Abort current executing command → IDLE |
| POST | `/sessions/:id` | `?action=yolo` `{"enabled":true}` | `Session` | Toggle YOLO mode |
| GET | `/sessions/:id/commands` | query: `?limit=50` | `CommandCard[]` | List past command cards, newest first |

### WebSocket

`GET /api/sessions/:id/stream` → upgrade to WebSocket

**Purpose**: stream terminal ANSI output plus command-card updates for monitor view. REST still provides initial/session fetches and command history endpoints.

**Server → Client messages:**
```json
{"type":"terminal","data":"<base64-encoded-ansi-bytes>"}
{"type":"snapshot","data":"<base64-capture-pane-content>"}
{"type":"card","card":{ /* CommandCard */ }}
```
`terminal` = live stream bytes. `snapshot` = full capture-pane on join.

`card` = command lifecycle update (`pending`, `executing`, `completed`, `rejected`, `aborted`) for monitor card list.

Implementation note (2026-06-01): for remote late-join stability, SS may send a bounded replay of recent FE terminal bytes immediately after `snapshot` for additional viewers. Replay uses the same `terminal` message shape.

**Client → Server**: All messages ignored (read-only terminal enforcement at SS level).

### CommandCard Schema

```json
{
  "id": "uuid",
  "command": "npm test",
  "state": "pending|executing|completed|rejected|aborted",
  "exit_code": 0,
  "output": "...",
  "created_at": "2026-05-28T10:00:00Z",
  "resolved_at": "2026-05-28T10:00:05Z",
  "resolved_by": "human|yolo"
}
```

### Session Schema

```json
{
  "id": "uuid",
  "name": "dev",
  "host": "",
  "state": "idle|pending|executing",
  "yolo": false,
  "current_command": null | { /* CommandCard fields */ },
  "created_at": "2026-05-28T10:00:00Z"
}
```

### Essential Dependencies

```
shush-core:
  serde, serde_json (types)
  uuid = { version = "1", features = ["v4", "serde"] }
  chrono = { version = "0.4", features = ["serde"] }
  thiserror (error types)
  tracing, tracing-subscriber

shush-bin:
  shush-core
  thiserror (error types)
  anyhow
  tokio = { version = "1", features = ["full"] }
  axum = { version = "0.8", features = ["ws"] }
  clap = { version = "4", features = ["derive"] }
  tower-http = { version = "0.6", features = ["cors", "fs"] }
  tracing, tracing-subscriber
  reqwest = { version = "0.12", features = ["json", "ws"] }
  base64 (for terminal data encoding in WebSocket)
```

May include more if see fit.



## Implementation Phases

### Phase 1: Project Scaffolding (1-2h)

Create workspace structure, Cargo.toml files, cli.rs with subcommands, Vite + TS frontend scaffold.

Files:
- `Cargo.toml` (workspace with members)
- `crates/shush-core/Cargo.toml` + `src/lib.rs` (empty)
- `crates/shush-bin/Cargo.toml` + `src/main.rs` (just clap dispatch)
- `crates/shush-bin/src/cli.rs` (ServerCmd, ClientCmd, SessionSubcommand)
- `frontend/package.json`, `tsconfig.json`, `vite.config.ts`, `index.html`
- `frontend/src/main.ts` (placeholder)

Verification:
```bash
cargo build
cd frontend && npm install && npm run build
```

### Phase 2: Core Types & tmux Event Parser (2-3h)

`shush-core` module:

`session.rs`:
- `SessionState` enum: `Idle`, `Pending`, `Executing`
- `CommandState` enum: `Pending`, `Executing`, `Completed(i32)`, `Rejected`, `Aborted`
- `CommandCard` struct
- `Session` struct: `{ id, name, host, state, yolo, created_at }`

`tmux_event.rs`:
- `TmuxEvent` enum: `Begin(u64, u64, u32)`, `End(u64, u64, u32)`, `Error(u64, u64, u32, String)`, `Output { pane: String, data: Vec<u8> }`, `WindowAdd(String)`, `SessionChanged(String, u64)`, `Unknown(String)`
- `parse_tmux_event(line: &str) -> TmuxEvent` — parse `%begin`, `%end`, `%error`, `%output %<pane> <octal>`, `%window-add`, `%session-changed`
- Octal-escaped byte decoder (Control Mode escapes control chars to octal)

`marker.rs`:
- `MarkerInjector::new() -> Self` — initializes CSPRNG
- `MarkerInjector::inject(command: &str) -> (String, Nonce)` — wraps command with start/end APC markers
- `MarkerDetector::feed(bytes: &[u8]) -> Vec<MarkerEvent>` — scans byte stream, detects start/end markers with valid nonce
- `Nonce` — 32-byte random value, hex-encoded

Verification:
```bash
cargo test -p shush-core
```

### Phase 3: Session Manager & Command Pipeline (4-5h)

`session_manager.rs`:
- `SessionManager` struct: `Arc<RwLock<HashMap<Uuid, Session>>>`
- `create(name, host) -> Session` — create logical session, ensure backing tmux session exists, size window for monitor stability
- `delete(id)` — kill tmux session, remove
- `get(id) -> Session`
- `list() -> Vec<Session>`
- owns current command plus completed/rejected/aborted command history
- publishes card updates to subscribers

`tmux_control.rs`:
- `TmuxControlModeClient` struct:
  - `spawn(session_name: &str, session_host: &str) -> Result<Self>` — attaches a persistent PTY-backed `tmux -L shush -CC attach -t <name>` client to the existing local or SSH-backed tmux session
  - Manages child process stdin/stdout with Tokio
  - `read_event() -> Option<TmuxEvent>` — async read of parsed control-mode events
  - `send_keys(text: &str)` — writes literal text via control-mode `send-keys -l -- ...`
  - `inject_command(command: &str)` — wraps with markers and calls send_keys
  - `active_pane: Option<String>` — tracked from `%window-pane-changed` events
  - `abort()` — calls `send_keys("C-c")`, fallback to kill-pane

`fe_master.rs`:
- FE here means frontend-facing backend stream, not frontend TypeScript code
- `FeMasterRegistry` owns read-only tmux attach clients for browser viewers
- `TerminalOutputFilter` strips APC marker sequences and rewrites wrapped command echo to plain user command for browser rendering

`command_executor.rs`:
- executes approved command through persistent SC control-mode client
- uses `MarkerInjector` to wrap command
- waits for matching marker completion on SC `%output`
- extracts command output, exit code, and completes command in `SessionManager`
- on failure, marks command aborted and drops stale control-mode client from registry

Session lifecycle during create:
1. Generate UUID, create Session struct (Idle state)
2. Spawn `tmux -L shush new-session -d -s <name>`
3. Resize session window for monitor stability
4. Defer SC `tmux -L shush -CC attach -t <name>` until first approved command needs execution
5. Defer FE read-only attach until first browser viewer connects
6. Set session state to Idle

Verification:
```bash
cargo test -p shush-bin -- --test-threads=1
# Manual: session create, submit command, check state transitions
```

### Phase 4: REST API Server (3-4h)

`server.rs`:
- Axum router setup under `/api`
- Shared state: `SessionManager`, `FeMasterRegistry`, `TmuxControlModeRegistry`, `CommandExecutor`
- REST handlers for sessions, commands, approve/deny/yolo actions
- WebSocket handler for terminal stream + card updates
- Static frontend serving with SPA fallback for deep links like `/monitor/:sessionId`

WebSocket behavior in `server.rs`:
1. Get or spawn FE master connection: `tmux -L shush attach -t <session> -r` under PTY
2. Send `capture-pane` snapshot as `{"type":"snapshot",...}`
3. If needed, send bounded FE replay buffer after snapshot for late joiners
4. Forward FE stdout chunks as `{"type":"terminal","data":"..."}`
5. Subscribe to command card state changes, push `{"type":"card","card":...}`
6. On disconnect: start FE idle timeout before teardown

Verification:
```bash
cargo run -- server &
curl -X POST http://127.0.0.1:8100/api/sessions -H 'Content-Type: application/json' -d '{"name":"dev","host":""}'
curl http://127.0.0.1:8100/api/sessions
```

### Phase 5: Browser Frontend (5-6h)

`api.ts`:
- `createSession(name, host)`, `listSessions()`, `getSession(id)`, `deleteSession(id)`
- `sessionAction(sessionId, action, body?)` — generic action dispatcher
- `listCommands(sessionId, limit?)`
- `toggleYolo(sessionId, enabled)`
- WebSocket connection helper with auto-reconnect (exponential backoff, max 30s)

`types.ts`:
- TypeScript interfaces matching API schema

`dashboard/dashboard.ts`:
- Session list fetched from API
- Each session card shows: name, state (colored dot), YOLO badge, command count
- Create session form (name input + create button)
- Delete session button with confirmation
- Click session → navigate to monitor view
- Poll every 5s (or long-poll, or we could push via shared WS, but polling is simpler for dashboard)

`terminal.ts`:
- Wraps xterm.js Terminal + FitAddon
- `write(data: Uint8Array)` — write to terminal
- `clear()` — reset terminal
- `resize(cols, rows)` — resize terminal (for canonical size matching)
- `fit()` — fit to container (FitAddon)
- Initialized to 220×50

`monitor/monitor.ts`:
- Route: `/monitor/:sessionId`
- Connects WebSocket to `/api/sessions/:id/stream`
- On `snapshot` message: `terminal.clear()`, `terminal.write(data)`
- On `terminal` message: `terminal.write(data)` — live stream
- On `card` message: update card list
- On reconnect: re-subscribe, expect new snapshot
- YOLO toggle button

`monitor/card_list.ts`:
- Command card list, newest first, max 50 items
- Each card: command text (monospace), state badge, timestamp, output preview (expandable)
- Pending cards: [Approve] [Deny] buttons
- Executing cards: [Abort] button
- Completed cards: exit code badge (green/red), output expand
- Rejected cards: strikethrough command text
- Auto-scroll to new cards

Styling: clean, functional, no framework dependency beyond xterm.js. Custom CSS, dark theme.

Verification:
```bash
cd frontend && npm run dev &
cargo run -- server &
# Open http://127.0.0.1:8100 in browser
# Create session, submit command, approve, see output in terminal
```

### Phase 6: SC CLI (2-3h)

`client/mod.rs` + `client/api_client.rs`:

- `shush client submit <session-id> <command>` — POST to session with `?action=submit`, wait for completion, stream output to stdout
  - Optional `--yolo` flag to bypass pending (but session must have YOLO enabled, or it gets queued as pending)
  - Stream terminal output via WebSocket while waiting
  - Print exit code on completion
  - Timeout: configurable (default 300s)
- `shush client sessions` — list sessions, their states, YOLO status
- `shush client yolo <session-id> <on|off>` — toggle YOLO
- `shush client stream <session-id>` — connect WebSocket, print terminal output + card updates to stdout (for CI/scripts)

`api_client.rs`:
- Reqwest-based HTTP client
- WebSocket connection with tokio-tungstenite
- JSON deserialization

Verification:
```bash
cargo run -- client sessions
cargo run -- client submit <session-id> "echo hello"
```


## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Language | Rust (SS/SC), TypeScript (FE) | Rust matches domain (SSH, tmux, concurrency); TS matches frontend ecosystem |
| HTTP framework | Axum | Best Tokio integration, built-in WebSocket support in v0.8 |
| tmux access | Direct tmux CLI (no SSH for local) | Avoids sshd dependency on local machine |
| Marker nonce | 32-byte CSPRNG, hex-encoded | Prevents prompt spoofing, no sequential guessability |
| Terminal encoding | Base64 in WebSocket JSON | Simplifies JSON parsing, avoids binary frame complexity |
| FE connection lifecycle | Dynamic (spawn on first viewer, teardown after 5s idle) | Conserves resources when no one is watching |
| FE attach transport | PTY-backed `tmux attach -r` | Runtime validation on tmux 3.6b/macOS showed plain piped stdio exits with `open terminal failed: not a terminal` |
| Late-join strategy | capture-pane snapshot + live stream | Works with tmux native features, no byte buffer needed for v0.1 |
| Abort strategy | SIGINT → kill-pane (2s timeout) | Standard tmux abort path, covers stuck processes |
| Port | Default 8100, override via `shush server --port <PORT>` | Keeps local dev UX simple while allowing ephemeral-port E2E isolation |

## Unknowns

- **tmux version compatibility**: Minimum tmux version needed? 3.x (2019+) for current Control Mode features. OS ships 3.4+ on modern macOS/Linux. Deferred: test on tmux 2.9+ and document minimum.
- **Scrollback in xterm.js**: capture-pane only gets viewport, not scrollback buffer. The live stream builds scrollback from the point of connection. Prior output is lost. Acceptable for v0.1.
- **Multiple panes**: Design says one window/one pane. If command spawns subprocesses that split, SS tracks active pane. For v0.1, document the restriction and handle gracefully (ignore extra pane events).

## Verification & Acceptance

### Automated
```bash
cargo test                    # unit + integration tests
cargo clippy -- -D warnings   # lint
cargo build --release
```
```
1. Start SS: cargo run -- server
   → Listen on http://127.0.0.1:8100

2. Create session:
   curl -X POST http://127.0.0.1:8100/api/sessions \
     -H 'Content-Type: application/json' \
     -d '{"name":"test","host":""}'
   → Returns session with id="<uuid>", state="idle"

3. Submit a command:
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=submit' \
     -H 'Content-Type: application/json' \
     -d '{"command":"echo hello"}'
   → Session shows current_command in PENDING state

4. Approve the command:
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=approve'
   → Session.current_command transitions to EXECUTING

5. Open browser: http://127.0.0.1:8100
   → Dashboard shows session, click to monitor
   → Terminal shows command output
   → Command list shows completed card with exit code

6. YOLO mode:
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=yolo' \
     -H 'Content-Type: application/json' \
     -d '{"enabled":true}'
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=submit' \
     -H 'Content-Type: application/json' \
     -d '{"command":"echo yolo"}'
   → Skips PENDING, goes straight to EXECUTING

7. Abort:
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=submit' \
     -d '{"command":"sleep 60"}'
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=approve'
   curl -X POST 'http://127.0.0.1:8100/api/sessions/<uuid>?action=abort'
   → Terminal shows "^C", session back to IDLE

8. List command history:
   curl 'http://127.0.0.1:8100/api/sessions/<uuid>/commands?limit=10'
   → JSON array of past command cards

9. Delete session:
   curl -X DELETE 'http://127.0.0.1:8100/api/sessions/<uuid>'
   → tmux session killed, removed from SS
```

## Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| tmux -CC output parsing breaks with tmux version changes | SS can't read command output | Pin minimum tmux version, integration test with CI |
| FE connection stdout buffer fills and blocks | Terminal freezes for all viewers | Use Tokio buffered reader, non-blocking reads |
| Memory leak from command card history | SS OOM after many commands | Fixed-size window (100 cards), oldest dropped |
| Race condition: approve + deny same card simultaneously | Double execution | State machine guards: approve checks PENDING, deny checks PENDING |
| Nonce collision | Wrong command considered complete | 32-byte CSPRNG: collision probability negligible (2^-256) |

## Dependencies

- **tmux 3.0+** (required on target machine)
- **No other external services**
- **No API keys or third-party accounts**
- **No MCP servers** — SC is a standalone CLI, not an MCP plugin for v0.1 (MCP integration can be added later)

## Out of Scope for This Plan

- MCP server integration (SC can be wrapped by MCP later)
- Docker/container packaging
- Native desktop notifications
- Authentication / user management
- Mobile-responsive frontend
- Tests for frontend (manual verification for v0.1)
