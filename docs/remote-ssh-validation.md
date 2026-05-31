# Remote SSH Validation (Issue 05)

This document describes the local Ubuntu Docker target used to validate `host = "user@host"` remote session behavior.

## What It Validates

- Remote session creation via `POST /api/sessions` with `host: "shush-docker"`
- Remote tmux lifecycle commands through SSH:
  - `has-session`
  - `new-session -d`
  - `capture-pane`
  - `kill-session`
- Session deletion removes the remote tmux session

## What It Does Not Validate

- Full remote command execution pipeline (`tmux -CC attach` path)
- Full remote browser stream behavior in all desktop/browser environments

## Remote Monitor Stream Status

- Remote FE monitor stream now has dedicated backend coverage under `server.rs` tests:
  - remote WebSocket connect receives `snapshot`
  - remote live output receives `terminal` frames
- Remote browser E2E remains environment-gated (`SHUSH_E2E_REMOTE=1`) because it requires the Docker SSH target and local browser runtime prerequisites.

## Prerequisites

- Docker
- `ssh`, `ssh-keygen`
- Rust toolchain (`cargo`)
- `curl`
- `python3` (only used for parsing JSON in script)

## Run Validation

```bash
chmod +x scripts/validate_remote_ssh.sh
./scripts/validate_remote_ssh.sh
```

If successful, the script prints:

```text
Remote SSH validation passed
```

## Docker Target Notes

- Docker image source: `docker/remote-ssh/Dockerfile`
- Entry point: `docker/remote-ssh/entrypoint.sh`
- SSH user inside container: `shush`
- SSH key auth is configured from `AUTHORIZED_KEY` env var
- Script uses local SSH config alias `shush-docker` in `.remote-ssh-home/.ssh/config`

## Platform Notes

- Local machine: macOS
- Remote target: Ubuntu 24.04 container
- One-shot SSH tmux commands work with this setup for issue 05 scope.
- Full remote PTY-backed attach behavior (`ssh ... tmux -CC attach`) is outside this issue's acceptance scope and should be validated separately when remote command streaming is implemented.
