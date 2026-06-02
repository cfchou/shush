# Monitor browser E2E coverage

Status: ready-for-agent

Priority: high - browser-level integration coverage for monitor flow

## What to build

Add browser-level end-to-end coverage for the monitor flow so shush verifies the real user-visible behavior of terminal monitoring in a live browser.

This issue should validate the **existing** monitor experience end-to-end, not redesign it.

**Test harness:**
- Use the project's chosen browser automation path to drive a real browser against local shush server
- Reuse the existing manual acceptance flow from issue 03 as the basis for automated assertions
- Keep the harness focused on monitor behavior; do not expand this issue into broad frontend test infrastructure work

**Coverage to add:**
- Open `/monitor/:sessionId` and verify the page loads from a deep link
- Verify the terminal view hydrates from the initial `snapshot` message
- Send visible output into the tmux session and verify the browser reflects the live update
- Open a second browser tab on the same session and verify both tabs can connect simultaneously
- Close and reopen a monitor tab and verify the page reconnects successfully
- Verify monitor navigation back to dashboard works

**Reconnect coverage:**
- Force a WebSocket disconnect or server restart during the test
- Verify the monitor reconnects with the existing retry behavior
- Verify the terminal is rehydrated from a fresh snapshot after reconnect

**Artifacts:**
- Capture screenshots on failure
- If the chosen harness supports it cheaply, capture trace/video only when a test fails

## Acceptance criteria

- [x] Browser automation can open `/monitor/:sessionId` directly and load the monitor page successfully
- [x] Initial tmux terminal state is visible in the browser after connect
- [x] New tmux output appears in the browser terminal during the same test run
- [x] Two browser tabs can connect to the same session successfully
- [x] Reconnect after close or temporary server disruption succeeds and rehydrates terminal state
- [x] Browser navigation back to dashboard works
- [x] Failure artifacts are preserved for debugging

## Blocked by

- Slice 03: FE master connection + terminal stream (`docs/issues/03-fe-master-terminal-stream.md`)

## References

- Issue 03: `docs/issues/03-fe-master-terminal-stream.md`
- Plan: `docs/shush-v01-plan.md`
