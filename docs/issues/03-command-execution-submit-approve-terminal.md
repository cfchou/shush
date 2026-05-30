# Command execution: submit, approve, terminal output

Status: ready-for-agent

## What to build

Implement the full command pipeline — submit a command, approve/deny it, watch execution and output in the browser terminal. This is the core value of shush.

**command_queue.rs:**
- `CommandQueue` struct: per-session FIFO queue, at most one command in PENDING or EXECUTING
- `submit(command: String) -> CommandCard` — create card in PENDING, enqueue
- `process_next()` — if current is PENDING and session is IDLE, execute
- `execute()` — calls `TmuxControlModeClient::inject_command` (wraps with markers via MarkerInjector), transitions to EXECUTING
- `approve()` — promote PENDING → EXECUTING
- `deny()` — clear current command, set REJECTED, session back to IDLE
- Marker detection callback: when end marker found via MarkerDetector, transition to IDLE, store exit code + output, call process_next

**REST endpoints:**
- `POST /api/sessions/:id?action=submit` — validate session, create CommandCard, submit to queue
- `POST /api/sessions/:id?action=approve` — validate PENDING state
- `POST /api/sessions/:id?action=deny` — validate PENDING state
- `GET /api/sessions/:id/commands?limit=50` — list past command cards

**WebSocket handler + FE master:**
- `ws.rs` — Axum WebSocket upgrade at `GET /api/sessions/:id/stream`
- On connect:
  1. Spawn FE master: `tmux -L shush attach -t <session> -r`
  2. Send capture-pane snapshot: `{"type":"snapshot","data":"<base64>"}`
  3. Fork reader task: read FE stdout in chunks, broadcast as `{"type":"terminal","data":"<base64>"}`
  4. Subscribe to command card state changes, push `{"type":"card","card":...}`
- On disconnect: close FE connection with idle timeout (5s before full teardown)

**Frontend: monitor view:**
- `monitor/monitor.ts` — route `/monitor/:sessionId`, WebSocket connection with auto-reconnect
- On `snapshot`: clear terminal, write snapshot data
- On `terminal`: write live stream data to terminal
- On `card`: update card list
- YOLO toggle button (placeholder action — actual YOLO routing in Slice 05)
- `monitor/terminal.ts` — xterm.js wrapper with FitAddon, initialized 220×50
- `monitor/card_list.ts` — command card list, newest first, max 50
  - Pending: command text + [Approve] [Deny] buttons
  - Executing: command text + [Abort] button (abort action in Slice 04)
  - Completed: exit code badge (green/red), expandable output
  - Rejected: strikethrough command text
- `monitor/monitor.css` — dark theme styling

## Acceptance criteria

- [ ] Submit a command via curl: `POST /api/sessions/:id?action=submit -d '{"command":"echo hello"}'` returns session with current_command in PENDING
- [ ] Approve via curl: `POST /api/sessions/:id?action=approve` transitions to EXECUTING
- [ ] Deny via curl: `POST /api/sessions/:id?action=deny` transitions back to IDLE, card shows REJECTED
- [ ] Browser monitor view connects via WebSocket, shows terminal output as command executes
- [ ] Completed card appears in card list with exit code
- [ ] `GET /api/sessions/:id/commands?limit=10` returns JSON array of past cards
- [ ] Auto-reconnect on WS disconnect (exponential backoff, max 30s)
- [ ] FE master tears down after 5s idle when last viewer disconnects

## Blocked by

- Slice 02: Session lifecycle create/list/dashboard (`.scratch/shush-v01/issues/02-session-lifecycle-create-list-dashboard.md`)

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 3 command_queue, Phase 4 ws.rs + actions, Phase 5 monitor)
