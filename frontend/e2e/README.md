# Monitor E2E

Run from `frontend/`:

```bash
npm run test:e2e
```

What this does:
- Builds frontend assets (`npm run build`)
- Starts `cargo run -- server`
- Runs Playwright monitor-browser tests

## Remote session coverage

Remote monitor coverage is included in the spec but gated behind env flags so local runs without a Docker SSH target remain deterministic.

Enable remote coverage after setting up issue-05 remote target:

```bash
SHUSH_E2E_REMOTE=1 npm run test:e2e
```

Optional overrides:
- `SHUSH_E2E_REMOTE_HOST` (default: `shush-docker`)
- `SHUSH_SSH_CONFIG` (default: `<repo>/.remote-ssh-home/.ssh/config`)
- `SHUSH_E2E_REMOTE_HOME` (default: `<repo>/.remote-ssh-home`)

## Stream-content assertions

By default, E2E verifies browser flow and reconnect lifecycle while avoiding fragile stream-content checks in environments where headless browser WS instrumentation is unstable.

Enable strict stream-content assertions with:

```bash
SHUSH_E2E_ASSERT_STREAM=1 npm run test:e2e
```

When enabled, tests assert that snapshot/terminal payloads observed in-browser include expected sentinels.
