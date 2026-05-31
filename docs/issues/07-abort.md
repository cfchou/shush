# Command abort

Status: ready-for-agent

## What to build

Add abort capability to kill a running command. When a user aborts, the server sends Ctrl+C via tmux send-keys, and falls back to `kill-pane` if the process doesn't stop within 2 seconds.

**abort.rs:**
- `abort_command(client: &TmuxControlModeClient, pane: &str)` function
  - Send `C-c` via `client.send_keys("C-c")`
  - Wait up to 2s for the marker's end event
  - If no end event in 2s, run `tmux -L shush kill-pane -t <pane>` as fallback

**REST:**
- `POST /api/sessions/:id?action=abort` — validate EXECUTING state, call abort, set card to ABORTED, session back to IDLE

**Frontend:**
- Executing command cards show [Abort] button (already wired in Slice 03's card_list, just needs the action wired)
- Card transitions to aborted state visually

## Acceptance criteria

- [ ] Submit a long-running command (`sleep 60`), approve, then abort via curl: card transitions to ABORTED, session back to IDLE
- [ ] Browser abort button triggers the same action
- [ ] Terminal shows `^C` or equivalent signal indication
- [ ] SIGINT fallback to kill-pane works if process ignores SIGINT
- [ ] Aborting an already-idle session returns error (validates EXECUTING state)

## Blocked by

- Slice 03: Command execution submit/approve/terminal (`.scratch/shush-v01/issues/03-command-execution-submit-approve-terminal.md`)

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Abort strategy in Key Decisions, abort.rs)
