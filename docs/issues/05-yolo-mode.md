# YOLO mode

Status: ready-for-agent

## What to build

Add per-session YOLO mode that bypasses the approval gate. When YOLO is enabled, submitted commands skip PENDING and go straight to EXECUTING.

**Server-side:**
- Session struct already has `yolo: bool` — just needs the toggle and submit logic
- `POST /api/sessions/:id?action=yolo` with body `{"enabled":true}` — toggles the flag on the session
- In `submit`: if `session.yolo == true`, skip PENDING, go directly to EXECUTING (call `execute()` instead of just queuing)
- Card's `resolved_by` set to `"yolo"` instead of `"human"`

**Frontend:**
- Monitor view: YOLO toggle switch/button
- When YOLO is on, submitted commands execute immediately (no approve/deny buttons shown)
- YOLO badge on session cards in dashboard

**SC CLI:**
- `shush client yolo <session-id> on|off` — toggle via REST

## Acceptance criteria

- [ ] Toggle YOLO on via curl: `POST /api/sessions/:id?action=yolo -d '{"enabled":true}'` sets `yolo: true`
- [ ] Submit a command with YOLO enabled: goes straight to EXECUTING (skips PENDING)
- [ ] Card shows `resolved_by: "yolo"`
- [ ] Browser YOLO toggle toggles the flag and shows current state
- [ ] `shush client yolo <id> on` works from CLI

## Blocked by

- Slice 03: Command execution submit/approve/terminal (`.scratch/shush-v01/issues/03-command-execution-submit-approve-terminal.md`)

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (YOLO mode throughout, api/yolo.rs)
