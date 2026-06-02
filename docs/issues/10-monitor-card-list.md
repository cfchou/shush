# Monitor card list (Frontend)

Status: ready-for-agent

Priority: medium

## What to build

**Frontend only.** Add the command card list to the monitor view, wired to the WS card push and REST actions.

**monitor/card_list.ts:**
- Command card list component, renders newest first, max 50 cards
- Each card displays:
  - Command text (monospace, pre-wrapped)
  - State badge: PENDING (yellow), EXECUTING (blue), Completed (green), REJECTED (red strikethrough), ABORTED (gray)
  - Timestamp (created_at, relative time like "2s ago")
  - Exit code badge for completed cards (green for 0, red for non-zero)
  - Expandable output section for completed cards (click to show/hide)
- Action buttons:
  - Pending cards: [Approve] [Deny] buttons → calls `POST ?action=approve` / `?action=deny`
  - Executing cards: [Abort] button → calls `POST ?action=abort`
- (Abort button is wired here, actual abort logic from Issue 12)
- Auto-scroll to newest card when added
- Visual transition animation (optional, CSS transition)

**monitor/monitor.ts update:**
- On `{"type":"card","card":{...}}` WS message: update card list (add new card, update existing card state)
- Initial load: fetch `GET /api/sessions/:id/commands?limit=50` on mount
- Layout: terminal on left (~70% width), card list on right (~30%) — responsive CSS

**monitor/monitor.css update:**
- Two-column layout: terminal left, cards right
- Card styling: dark theme, border, hover effects
- State badge colors
- Button styling (approve=green, deny=red, abort=orange)
- Output expandable section styling

**api.ts update:**
- `sessionAction(sessionId, action, body?)` — generic action dispatcher
- `listCommands(sessionId, limit?)` — fetch command history

## Acceptance criteria

- [ ] Monitor view shows card list on the right side
- [ ] New commands appear as pending cards with Approve/Deny buttons
- [ ] Clicking Approve calls API, card transitions to EXECUTING (blue badge, Abort button)
- [ ] Clicking Deny calls API, card transitions to REJECTED (strikethrough)
- [ ] Completed cards show exit code badge and expandable output
- [ ] Card list auto-scrolls to newest
- [ ] Cards persist in list across page reload (fetched via GET /commands)

## Blocked by

- Issue 03: FE master + terminal stream (`docs/issues/03-fe-master-terminal-stream.md`) — provides WS channel and monitor view layout
- Issue 09: Command queue + REST (`docs/issues/09-command-queue-submit-approve-deny.md`) — provides REST endpoints and WS card push

## References

- Plan: `docs/shush-v01-plan.md` (Phase 5 card_list.ts, Phase 4 actions + sessions)
- Also update `api.ts` to add `sessionAction()` and `listCommands()`
