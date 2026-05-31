# 03 — FE master connection + terminal stream

Status: ready-for-agent

Priority: HIGHEST — FE must connect and see the tmux session

## What to build

Implement the FE master connection lifecycle and WebSocket terminal stream, so the browser can connect and see live tmux output.

**fe_master.rs:**
- `FeMaster` struct — manages a `tmux -L shush attach -t <session> -r` child process per session
- `spawn(session_name) -> FeMasterHandle` — spawns read-only tmux attach, returns handle with:
  - `snapshot()` — runs `capture-pane -t <session>` and returns base64-encoded content
  - Broadcast channel sender for stdout bytes to all connected WS viewers
  - Idle timeout (5s after last viewer disconnects, then kill child process)
  - New connect during idle timer → cancel timer, keep FE master alive
- Reader task (tokio::spawn): reads FE stdout in chunks, sends to broadcast channel

**ws.rs:**
- `ws_handler` — Axum WebSocket upgrade at `GET /api/sessions/:id/stream`
- On connect:
  1. Get or create FeMaster for this session
  2. Send capture-pane snapshot: `{"type":"snapshot","data":"<base64>"}`
  3. Subscribe to broadcast channel, forward chunks as `{"type":"terminal","data":"<base64>"}`
  4. On disconnect: signal FeMaster to start idle timer
- All Client→Server messages ignored (read-only enforcement)

**Frontend — monitor/terminal.ts:**
- Wraps xterm.js Terminal + FitAddon
- `write(data: Uint8Array)` — write decoded base64 to terminal
- `clear()` — reset terminal
- `fit()` — fit to container
- Initialized to 220×50, dark theme

**Frontend — monitor/monitor.ts:**
- Minimal route at `/monitor/:sessionId`
- WebSocket connect to `/api/sessions/:id/stream`
- On `snapshot`: `terminal.clear()`, `terminal.write(snapshot)`
- On `terminal`: `terminal.write(data)` — live stream
- Auto-reconnect with exponential backoff (1s, 2s, 4s… max 30s)
- On reconnect: expect new snapshot from server
- No card list yet — pure terminal view
- Navigation back to dashboard

**Frontend — monitor/monitor.css:**
- Dark theme, full-height terminal layout

## Acceptance criteria

- [x] Open browser at `/monitor/:sessionId` for a running session, see the tmux terminal in xterm.js
- [x] Commands typed in tmux (via SC or direct) appear in the browser terminal in real-time
- [x] Disconnect/reconnect: WS auto-reconnects with backoff, new snapshot is sent
- [x] FE master spawns on first WS connect
- [x] FE master tears down 5s after last WS viewer disconnects
- [x] Multiple browser tabs can view same session simultaneously
- [x] Timed out FE master re-spawns on new WS connect

## Blocked by

- Slice 02: Session lifecycle create/list/dashboard (`docs/issues/02-session-lifecycle-create-list-dashboard.md`)

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 4 ws.rs, Phase 5 terminal.ts + monitor.ts)
- FE connection lifecycle: plan Key Decisions table, FE connection row
