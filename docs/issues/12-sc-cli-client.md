# SC CLI client

Status: ready-for-agent

Priority: medium

## What to build

Build the `shush client` subcommands so agents and scripts can interact with shush server from the command line, without a browser.

**client/mod.rs:**
- Module setup, imports

**client/api_client.rs:**
- Reqwest-based HTTP client for all REST endpoints
- WebSocket connection (tokio-tungstenite) for terminal streaming
- JSON deserialization into `Session`, `CommandCard` types

**CLI subcommands (wired into cli.rs):**

`shush client submit <session-id> <command>`:
- POST submit to session
- If pending (non-YOLO), wait for approval (poll or WS)
- If executing, stream terminal output via WebSocket to stdout
- Print exit code on completion
- Optional `--yolo` flag (bypasses pending — requires session YOLO enabled)
- Configurable timeout (default 300s)

`shush client sessions`:
- List all sessions with their states, YOLO status, host

`shush client stream <session-id>`:
- Connect to WebSocket, print terminal output + card updates to stdout
- Useful for CI/scripts

`shush client yolo <session-id> <on|off>`:
- Toggle YOLO mode on a session

## Acceptance criteria

- [ ] `cargo run -- client sessions` lists sessions from the server
- [ ] `cargo run -- client submit <id> "echo hello"` submits, streams output, prints exit code
- [ ] `cargo run -- client submit --yolo <id> "echo yolo-cli"` submits with YOLO bypass
- [ ] `cargo run -- client stream <id>` connects WebSocket and prints terminal output + card events
- [ ] `cargo run -- client yolo <id> on` toggles YOLO on
- [ ] Timeout on submit (300s default) terminates gracefully

## Blocked by

- Slice 03: Command execution submit/approve/terminal (`.scratch/shush-v01/issues/03-command-execution-submit-approve-terminal.md`)

## References

- Plan: `docs/shush-v01-implementation-plan-2026-05-28-approved.md` (Phase 6)
