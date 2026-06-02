# Host semantics: only empty host is local; `localhost` must use SSH

Status: ready-for-agent

Priority: high

## What to build

Fix host backend selection so shush no longer treats `localhost` as a reliable signal for direct local tmux.

Today, shush uses a local-only optimization where both `""` and `"localhost"` are treated as direct local tmux execution. This is incorrect when a user intentionally uses SSH with a loopback endpoint, such as:

- SSH port forwarding to a remote machine exposed on local loopback
- a local SSH config alias that resolves to a forwarded or tunneled target
- any remote workflow where `localhost` is the SSH endpoint but the desired tmux lives on the remote side

In those cases, shush runs the local machine's tmux instead of the intended remote tmux.

The required semantic rule is:

- `host == ""` => direct local tmux
- any non-empty `host` => SSH-backed tmux

This issue is about correcting that rule consistently across the runtime and docs.

**Backend behavior:**
- Update the shared host-selection logic so only the empty host means local direct tmux
- Treat `localhost`, `127.0.0.1`, and any non-empty alias as SSH targets
- Ensure all runtime call sites use the same rule:
  - session create / has-session
  - session delete
  - `capture-pane`
  - FE attach / snapshot paths

**Tests:**
- Add focused tests for the shared host-selection rule
- Verify at minimum:
  - empty host is local
  - `localhost` is remote
  - a non-empty alias such as `shush-docker` is remote

**Docs:**
- Update any docs that currently claim `""`/`"localhost"` are both local
- Explicitly document why `localhost` cannot be used as a shortcut for local direct tmux

## Acceptance criteria

- [ ] Only `host == ""` is treated as direct local tmux
- [ ] `host == "localhost"` is treated as SSH-backed tmux
- [ ] Remote validation still passes when using a non-empty SSH alias host such as `shush-docker`
- [ ] Helper-level tests cover empty host vs `localhost` vs non-empty alias behavior
- [ ] No user-facing docs still describe `localhost` as direct local tmux

## Blocked by

- None

## References

- Plan: `docs/shush-v01-plan.md`
- Remote validation slice: `docs/issues/05-remote-ssh-ubuntu-docker-validation.md`
- Remote validation doc: `docs/remote-ssh-validation.md`
