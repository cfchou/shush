# Remote SSH validation against Ubuntu Docker target

Status: ready-for-agent

## What to build

Add a focused validation slice for remote-host behavior using an Ubuntu Linux Docker container running `sshd` and `tmux`, so shush can prove the remote path before more remote features land.

This issue is about **testable remote session bring-up**, not full remote feature expansion.

**Test target setup:**
- Add a reproducible Ubuntu Docker target for local testing
- Container must include:
  - `openssh-server`
  - `tmux`
  - a test user with SSH key auth
- Document how to start and stop the target locally

**Backend validation:**
- Exercise `host = "user@host"` remote session creation path against the Docker target
- Verify shush can create a tmux session remotely
- Verify shush can attach to the remote tmux session using the same socket/session conventions expected by the server
- Verify remote one-shot tmux commands used by shush still work:
  - `new-session -d`
  - `has-session`
  - `capture-pane`
  - `kill-session`

**Testing:**
- Add one or more explicit validation tests or scripts for the remote path
- Prefer a reproducible automated check over purely manual verification
- If full CI automation is too large for this slice, provide a single local verification command that brings the target up and runs the remote checks end-to-end

**Docs:**
- Document assumptions and any Linux-vs-macOS differences discovered during validation
- If the remote path requires different process spawning or PTY handling than local mode, capture that in an ADR update or new ADR

## Acceptance criteria

- [ ] A local Ubuntu Docker container can be started as an SSH target for shush development
- [ ] Shush can create a session with `host` pointing at that SSH target
- [ ] The remote tmux session is created successfully and can be confirmed on the container
- [ ] `capture-pane` works against the remote session and returns terminal content
- [ ] Remote session deletion removes the tmux session on the container
- [ ] A repeatable validation command or test script exists for this remote path
- [ ] Any PTY / tmux / SSH behavioral differences discovered are documented

## Blocked by

- Slice 02: Session lifecycle create/list/dashboard (`docs/issues/02-session-lifecycle-create-list-dashboard.md`)
- Remote execution path implementation for `host = "user@host"` where still incomplete

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Architecture: local + remote tmux sessions, `host` field behavior)
- ADR-0001: `docs/adr/0001-tmux-control-mode-pty.md`
- ADR-0002: `docs/adr/0002-xterm-js-raw-ansi-websocket.md`
