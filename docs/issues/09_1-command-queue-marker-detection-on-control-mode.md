# Command queue marker detection must move to tmux control-mode

Status: ready-for-agent

Priority: high

## Problem

Issue 09's submit/approve/deny REST flow is only partially validated.

- `submit`, `approve`, `deny`, command history listing, and WS card push are implemented.
- `cargo test` passes.
- A real remote browser scenario still breaks when a command is executed through the queue via `POST ?action=submit` + `POST ?action=approve`.

Observed behavior in monitor:

- Before fixing remote injection quoting, the browser terminal showed marker garbage like nonce fragments and `\033\\`.
- After fixing remote literal injection, the command now visibly runs and prints `hello`, but completion still times out and marker artifacts like `END_<nonce>...` can leak into the terminal.
- The session remains stuck from the command-queue point of view because completion is never detected reliably.

## Root cause

Historical root cause at time of filing:

The implementation then was trying to detect command-completion markers from FE monitor stream, but repo architecture docs said marker detection should happen on tmux control-mode `%output` stream instead.

Evidence:

- Historical code path at filing time:
  - `crates/shush-bin/src/command_executor.rs`
  - `wait_for_completion()` subscribes to `FeMaster` chunks and feeds them into `MarkerDetector`
- Architecture references:
  - `docs/adr/0003-tmux-ansi-stream-encoding.md`
    - FE stream is raw ANSI for browser rendering only
    - `%output` from control-mode is the stream SS should parse for markers
  - `docs/adr/0001-tmux-control-mode-pty.md`
    - marker-based command lifecycle depends on `%output`
  - `docs/shush-v01-plan.md`
    - session lifecycle calls for a persistent `tmux -CC attach` client whose `%output` drives command lifecycle

So there are two separate bugs/gaps:

1. Remote approved-command injection needed literal typing semantics.
2. Even after that, using FE bytes for marker detection is architecturally wrong and does not produce reliable completion detection.

## Current implementation note

This issue is now completed.

Current implementation:

- uses `crates/shush-bin/src/tmux_control.rs` control-mode `%output` for marker completion
- uses `crates/shush-bin/src/command_executor.rs` to wait for matching marker end events
- keeps backend FE stream in `crates/shush-bin/src/fe_master.rs` browser-facing only
- rewrites wrapped command echo for browser rendering and strips APC marker sequences before xterm render

## Reproduction

Manual reproduction:

1. Create a remote session on `shush-docker`
2. Open `/monitor/:sessionId`
3. `POST /api/sessions/:id?action=submit` with `{"command":"echo hello"}`
4. `POST /api/sessions/:id?action=approve`

Expected:

- terminal shows normal shell interaction for `echo hello`
- card reaches `Completed(0)`
- session returns to `IDLE`
- no marker garbage is visible in monitor

Actual:

- terminal may show `hello`
- command completion can still time out
- marker artifacts may still leak into monitor
- session/card lifecycle does not complete reliably

## Existing regression coverage

There is now a remote Playwright regression scenario in:

- `frontend/e2e/monitor.e2e.ts`

It covers:

- remote session monitor page
- terminal baseline capture before command
- submit + approve REST workflow
- before/after screenshots for Playwright artifacts
- assertions for visible command/output and absence of marker garbage

At filing time, this scenario was expected to fail until architectural fix below was implemented.

## What to build

Move command completion detection off the FE stream and onto a persistent tmux control-mode client per session.

### Backend changes

- Add or finish a per-session `TmuxControlModeClient` lifecycle owned by the session manager/server
- Keep the `-CC attach` client alive for the session so SS continuously receives `%output`
- Feed `%output` decoded bytes into `MarkerDetector`
- Drive command lifecycle transitions from control-mode marker events:
  - `Pending -> Executing`
  - `Executing -> Completed(exit_code)`
  - session back to `Idle`
  - history updated
  - WS card push emitted
- Keep FE stream for browser rendering only; it should not be the source of truth for command completion

### Command injection

- Preserve the remote literal injection hardening:
  - marker-wrapped commands must be typed literally into tmux
  - remote invocation must not mangle shell quoting/backslashes

### E2E / verification

- Make the remote monitor submit/approve Playwright scenario pass
- Ensure it verifies:
  - terminal before/after differs as expected
  - `echo hello` and `hello` are visible in monitor
  - no `_BEGIN_`, `_END_`, or `\\033\\` leaks remain visible
- Add or update backend tests for control-mode-driven marker completion if feasible

## Acceptance criteria

- [x] Remote submit + approve of `echo hello` reaches `Completed(0)` and session returns to `IDLE`
- [x] Browser monitor shows `echo hello` and `hello` without marker garbage
- [x] Marker detection uses tmux control-mode `%output`, not FE stream bytes
- [x] Remote monitor Playwright regression test passes with `SHUSH_E2E_REMOTE=1 SHUSH_E2E_ASSERT_STREAM=1`
- [x] `cargo test` passes

## Temporary Investigation Changes To Revisit

These changes were added primarily to surface and investigate this blocker. Keep them under review while implementing the real control-mode fix.

Likely to keep as regression coverage:

- `frontend/e2e/monitor.e2e.ts`
  - remote approve-flow regression test
  - before/after screenshots
  - assertions for visible command/output and absence of marker garbage

Historical notes from investigation phase:

- `crates/shush-bin/src/command_executor.rs`
  - at filing time had FE-stream-based `wait_for_completion(...)` path
- `crates/shush-bin/src/remote_tmux.rs`
  - remote shell-quoted tmux helper
  - remained useful during transition to persistent control-mode client

Support / strict-validation changes that may or may not remain after `09_1` is complete:

- `frontend/package.json`
- `frontend/package-lock.json`
  - `@types/node` added for Playwright/TS support code
- `Makefile` `e2e` target
  - currently forces `SHUSH_E2E_ASSERT_STREAM=1`
  - keep if strict monitor-stream validation should remain the default remote E2E mode
  - relax if that is too strict for the normal developer workflow

## Blocks / Related

- Blocks finishing `docs/issues/09-command-queue-submit-approve-deny.md`
- Related to `docs/issues/03-fe-master-terminal-stream.md`
- Backed by `docs/adr/0001-tmux-control-mode-pty.md`
- Backed by `docs/adr/0003-tmux-ansi-stream-encoding.md`
