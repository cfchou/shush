# FE unit tests: monitor WS lifecycle

Status: ready-for-agent

Priority: high

## What to build

Add focused unit-test coverage for `frontend/src/monitor/monitor.ts`, covering the WebSocket lifecycle and message handling behavior that currently has no direct unit protection.

This issue is **unit-test only**. Do not redesign monitor UX, and do not expand into broad frontend test infrastructure work.

### Scope

- Add tests for `renderMonitor()` behavior using Vitest
- Mock/stub browser primitives needed for deterministic tests:
  - `WebSocket`
  - `window.setTimeout`
  - `window.addEventListener` paths relevant to reconnect/unload
- Mock `TerminalView` so tests can assert interaction behavior (`clear`, `write`, `fit`) without depending on xterm internals

### Behaviors to cover

- Monitor shell renders with escaped session id text
- WebSocket URL is built correctly for:
  - `http:` page -> `ws://...`
  - `https:` page -> `wss://...`
- `open` event resets reconnect attempt counter
- `message` handling:
  - ignores non-string payloads
  - ignores invalid JSON
  - ignores messages with missing or non-string `data`
  - on `snapshot`: `terminal.clear()` then `terminal.write(bytes)` then `terminal.fit()`
  - on `terminal`: `terminal.write(bytes)` only
- `close` handling:
  - schedules reconnect via `setTimeout(connect, reconnectDelayMs(attempt))`
  - increments attempt between retries
- `error` handling closes the socket
- `beforeunload` marks closed state so subsequent `close` does not schedule reconnect

## Acceptance criteria

- [ ] `frontend/src/monitor/monitor.test.ts` includes unit tests for monitor WS lifecycle and message handling
- [ ] Tests assert protocol selection (`ws` vs `wss`) and session-id encoding in stream URL
- [ ] Tests verify snapshot vs terminal message side effects on terminal methods
- [ ] Tests verify reconnect scheduling and backoff progression after close events
- [ ] Tests verify no reconnect scheduling after `beforeunload`
- [ ] `cd frontend && npm test` passes

## Out of scope

- Browser-level multi-tab behavior
- Real server disconnect/restart validation
- Screenshot/trace artifact collection

Those belong in `docs/issues/07-monitor-browser-e2e.md`.

## References

- Frontend monitor implementation: `frontend/src/monitor/monitor.ts`
- Existing stream helpers: `frontend/src/monitor/stream_utils.ts`
- Existing minimal unit tests: `frontend/src/monitor/monitor.test.ts`, `frontend/src/monitor/terminal.test.ts`
- E2E monitor coverage issue: `docs/issues/07-monitor-browser-e2e.md`
