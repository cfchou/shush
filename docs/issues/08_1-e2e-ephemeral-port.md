# E2E ephemeral port for test isolation

Status: open

Priority: medium

## Problem

`frontend/e2e/monitor.e2e.ts` hard-codes `http://127.0.0.1:8100` for all HTTP requests and Playwright navigation. The backend server also binds to `8100` by default.

This creates two classes of noise in restart-heavy E2E coverage:

- **Restart races** — `runtime.restart()` stops the old process and starts a new one on the same port. If the OS has not released `8100` yet, the new server panics with `AddrInUse`.
- **Machine-local interference** — anything else using `8100` on the host (including a stray `shush-bin server` process) collides with the E2E run.

For E2E, a per-run ephemeral port is a better fit.

## Scope

- Allocate a free local port at E2E startup before starting the backend.
- Start the backend on that chosen port.
- Thread a shared `baseUrl` through all E2E helpers instead of hard-coding `8100`.
- Preserve existing monitor E2E coverage, including mid-test restart cases.

### Out of scope

- Changing the production default bind address or port.
- Redesigning the E2E harness beyond base URL threading.

## Investigation notes

- Check whether the Rust server already supports bind override through existing CLI args or environment variables.
- Prefer reusing existing bind configuration over adding an E2E-only knob.
- If no override exists, add the smallest possible bind override path (e.g. `SHUSH_BIND_ADDR` env var or `--bind` flag).

## Acceptance criteria

- [ ] E2E runtime no longer hard-codes port `8100`
- [ ] Backend binds to a per-run chosen port during E2E
- [ ] Helper requests and browser navigation use a shared runtime base URL
- [ ] Restart coverage passes without `AddrInUse` noise caused by fixed-port reuse
- [ ] `SHUSH_E2E_REMOTE=1 npm run test:e2e` remains green

## References

- Current E2E file: `frontend/e2e/monitor.e2e.ts`
- Server bind config: `crates/shush-bin/src/main.rs`
