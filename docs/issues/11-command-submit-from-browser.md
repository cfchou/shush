# Command submit from browser

Status: ready-for-agent

Priority: medium

## What to build

**Frontend only.** Add a command input field to the monitor view so users can type and submit commands directly from the browser.

**monitor/monitor.ts update:**
- Add command input bar at the bottom of the terminal area:
  - Text input field (monospace font, dark theme)
  - Submit button (or Ctrl+Enter / Enter shortcut)
  - Placeholder text: "Enter command..."
- On submit:
  1. Call `POST /api/sessions/:id?action=submit` with the command text
  2. Clear input field
  3. Disable input while session is in PENDING or EXECUTING state
  4. Re-enable when session returns to IDLE
- Session state tracking:
  - Track current session state (poll `GET /api/sessions/:id` or derive from WS card messages)
  - Disable input + show "Command in progress..." indicator when PENDING/EXECUTING
  - Enable input + show placeholder when IDLE

**monitor/monitor.css update:**
- Command input bar styling: bottom of terminal area, dark theme, monospace
- Disabled input state visual feedback (dimmed, no-cursor)
- Submit button styling

## Acceptance criteria

- [ ] Command input appears at the bottom of the terminal in monitor view
- [ ] Typing a command and pressing Enter submits it to the session
- [ ] Submitted command appears as a pending card in the card list
- [ ] Input is disabled while a command is pending or executing
- [ ] Input re-enables when session returns to IDLE
- [ ] Error handling: if submit fails (session deleted, etc.), show inline error message

## Blocked by

- Issue 09: Command queue + REST (`docs/issues/09-command-queue-submit-approve-deny.md`) — provides the submit REST endpoint
- Issue 10: Monitor card list (`docs/issues/10-monitor-card-list.md`) — users need to see the result of their submission

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 5 monitor.ts, no explicit submit-from-browser in plan but implied by dashboard-to-monitor flow)
