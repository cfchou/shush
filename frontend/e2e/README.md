# Monitor E2E

Run from `frontend/`:

```bash
npm run test:e2e
```

What this does:
- Builds frontend assets (`npm run build`)
- Starts `cargo run -- server`
- Runs Playwright monitor-browser tests

## E2E defaults and remote session coverage

Frontend E2E defaults in this repository now enable both remote mode and stream assertions:

- `SHUSH_E2E_REMOTE=1`
- `SHUSH_E2E_ASSERT_STREAM=1`

For remote monitor coverage, set up the SSH target first:

```bash
./scripts/remote_ssh_target.sh
npm run test:e2e
```

To run local-only E2E without remote session coverage, pass:

```bash
SHUSH_E2E_REMOTE=0 npm run test:e2e
```

Optional override:
- `SHUSH_E2E_REMOTE_HOST` (default: `shush-docker`)

## Stream-content assertions

By default, tests assert stream/snapshot payload expectations in-browser.

Disable it per run if needed with:

```bash
SHUSH_E2E_ASSERT_STREAM=0 npm run test:e2e
```

When enabled, tests assert that snapshot/terminal payloads observed in-browser include expected sentinels.
