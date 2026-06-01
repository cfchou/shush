# Remote FE monitor stream fix

Status: in-progress

Priority: high

## What to build

Fix the remote monitor streaming path so `/monitor/:sessionId` works for SSH-backed sessions the same way it already works for local sessions.

Today, remote session setup is partially validated:

- remote one-shot tmux commands work (`has-session`, `new-session -d`, `capture-pane`, `kill-session`)
- local browser monitor streaming works

But the long-lived remote FE attach path used by browser monitoring is a different runtime path and is not yet proven end-to-end.

The failing path is the FE master remote attach lifecycle in `crates/shush-bin/src/fe_master.rs`, where shush spawns a PTY-backed remote command equivalent to:

```bash
ssh -tt <host> tmux -L shush attach -r -t <session>
```

This issue is about making that remote FE attach/read/reconnect path work reliably.

### Scope

- Harden remote FE spawn behavior in `fe_master.rs`
- Add focused diagnostics and tests for the remote FE attach path
- Prove that remote browser monitor flow receives:
  - initial `snapshot`
  - live `terminal` updates
  - fresh snapshot again after reconnect/restart
- Update docs so remote browser monitor streaming is no longer described as outside the validated scope once the fix lands

### Behaviors to cover

- Remote FE attach spawn succeeds for an SSH-backed session
- Remote WebSocket connect receives `snapshot`
- Remote tmux output is forwarded as live `terminal` frames
- Remote FE idle teardown / reconnect still works
- Remote browser monitor E2E passes with `SHUSH_E2E_REMOTE=1`

## Acceptance criteria

- [x] Remote FE attach path is covered by focused backend tests, not only browser tests
- [x] Remote monitor WebSocket receives an initial snapshot for SSH-backed sessions
- [x] Remote live tmux output appears in the browser monitor flow for SSH-backed sessions
- [x] Remote reconnect after disconnect or server restart succeeds and rehydrates terminal state
- [x] `SHUSH_E2E_REMOTE=1 npm run test:e2e` passes from `frontend/`
- [x] Docs no longer state that full remote browser stream behavior is outside validated scope

## Progress update (2026-06-01)

### New failure mode identified

- Multi-tab monitor join could show a temporarily blank/incomplete second tab while first tab stayed healthy.
- Refresh/reconnect could temporarily lose status/prompt fidelity and recover later.

### Root cause (current understanding)

- Late-joining viewers were bootstrapped with `capture-pane` snapshot, which does not fully match FE-client-rendered frame semantics.
- FE lifecycle timing (reader EOF/close/reconnect) could leave viewers in transient stale states before later redraws.

### Mitigations implemented so far

- FE stream now emits close events and WS loops break on FE closure, forcing cleaner reconnect behavior.
- Dead FE handles are torn down and respawned explicitly.
- Additional viewer bootstrap path replays recent FE bytes from a bounded in-memory buffer.
- Remote session window size is pinned (`window-size manual`, `220x50`) to reduce attach/detach geometry drift.
- Frontend monitor handling improved with reconnect status, unload socket close, line-ending normalization, and explicit terminal repaint after writes.

### Remaining work

- Continue observing multi-tab behavior in manual dogfooding; no deterministic failure reproduced in latest remote E2E run.

## Blocked by

- Issue 05: Remote SSH validation against Ubuntu Docker target (`docs/issues/05-remote-ssh-ubuntu-docker-validation.md`)

## References

- Monitor browser E2E: `docs/issues/07-monitor-browser-e2e.md`
- Remote validation doc: `docs/remote-ssh-validation.md`
- FE stream implementation: `docs/issues/03-fe-master-terminal-stream.md`
- ADR-0002: `docs/adr/0002-xterm-js-raw-ansi-websocket.md`
