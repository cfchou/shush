# 04 — Command queue + submit/approve/deny REST

Status: ready-for-agent

Priority: high

## What to build

**Backend only.** Implement the command queue and REST actions for submit/approve/deny, plus push card state changes over the existing WebSocket.

**command_queue.rs:**
- `CommandQueue` struct: per-session FIFO queue, at most one command in PENDING or EXECUTING at a time
- `submit(command: String) -> CommandCard` — create CommandCard in PENDING state, set as current command on session
- `process_next()` — if current is PENDING and session is IDLE, call execute()
- `execute()` — calls `TmuxControlModeClient::inject_command()` (wraps with markers via MarkerInjector), transitions to EXECUTING, updates session state
- `approve()` — promote current command from PENDING to EXECUTING, call execute()
- `deny()` — set card to REJECTED, clear current command, session back to IDLE
- Marker detection integration: when `MarkerDetector` finds end marker via the FE master output stream:
  - Extract exit code from marker metadata
  - Set card to Completed(exit_code), collect accumulated output
  - Clear current command, session back to IDLE
  - Call process_next() for any queued command

**REST endpoints (add to api/sessions.rs or new api/commands.rs):**
- `POST /api/sessions/:id?action=submit` — body: `{"command":"..."}` — validate session exists, create card, submit to queue
- `POST /api/sessions/:id?action=approve` — validate PENDING state
- `POST /api/sessions/:id?action=deny` — validate PENDING state
- `GET /api/sessions/:id/commands?limit=50` — list past CommandCards, newest first

**WebSocket card push:**
- When card state changes (PENDING→EXECUTING, EXECUTING→COMPLETED, etc.), broadcast `{"type":"card","card":{...}}` on the session's WS broadcast channel
- No frontend handling yet — that comes in Issue 05

**Session schema update:**
- `Session` struct gains `current_command: Option<CommandCard>` field
- Session state reflects current command state: PENDING/EXECUTING while command active, IDLE when no command
- Session serialization includes current_command if present

## Acceptance criteria

- [ ] `POST ?action=submit -d '{"command":"echo hello"}'` returns session with `current_command` in PENDING
- [ ] `POST ?action=approve` transitions command to EXECUTING, session to EXECUTING
- [ ] `POST ?action=deny` transitions command to REJECTED, session back to IDLE
- [ ] Command execution completes normally: card reaches Completed(0), session back to IDLE
- [ ] Marker detection works: command output before/after markers is captured as card output
- [ ] `GET /api/sessions/:id/commands?limit=10` returns JSON array of past cards, newest first
- [ ] Card state changes are pushed as `{"type":"card"...}` messages on WS (visible in browser console)
- [ ] Queue permits at most one pending/executing command; submit during pending returns error or enqueues
- [ ] `cargo test` passes

## Blocked by

- Slice 02: Session lifecycle (`docs/issues/02-session-lifecycle-create-list-dashboard.md`)
- Issue 03: FE master + terminal stream (`docs/issues/03-fe-master-terminal-stream.md`) — marker detection reads from FE output stream

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 3 command_queue, Phase 4 actions + commands list)
- Marker protocol: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (marker.rs in Phase 2)
