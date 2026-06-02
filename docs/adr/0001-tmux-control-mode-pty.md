# ADR-0001: tmux Control Mode Requires a PTY on Every Platform

**Status:** Accepted  
**Date:** 2026-05-30  
**Deciders:** engineering  

---

## Context

shush maintains **two separate tmux client processes** per managed session:

| Connection | Command | Purpose |
|---|---|---|
| **SC connection** | `tmux -L shush -CC attach -t <session>` | Structured event stream (`%output`, `%begin`, `%end`, `%session-changed`); marker detection; `send-keys` for command injection |
| **FE connection** | `tmux -L shush attach -t <session> -r` | Raw ANSI byte stream read from a PTY-backed attach client, base64-encoded into `{"type":"terminal"}` WebSocket frames for xterm.js |

The FE connection uses plain `attach -r` (read-only, **no `-CC`**). Early investigation assumed this path would work headlessly with piped stdio and no PTY. Runtime verification on tmux `3.6b` on macOS contradicted that assumption: `attach -r` also exits immediately with `open terminal failed: not a terminal` when spawned without a tty. The FE connection therefore also needs a PTY, even though it does not use control mode.

This ADR originally concerned only the **SC connection** (`-CC`), but later FE runtime research showed the same practical PTY requirement for the read-only attach path used by browser viewers.

The original SC connection was spawned using `tokio::process::Command` with `Stdio::piped()`:

```rust
tokio::process::Command::new("tmux")
    .args(["-L", socket, "-CC", "attach", "-t", session_name])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .spawn()
```

This gave tmux a plain Unix pipe (not a terminal) as its stdin/stdout.

---

## Problem

Spawning the SC connection (`tmux -CC attach`) with piped stdin fails with:

```
tcgetattr failed: Inappropriate ioctl for device   # Linux
tcgetattr failed: Operation not supported by device # macOS
```

The process exits immediately. No control-mode events are ever emitted.

### Root cause: the source code

This is **not a macOS-specific bug**. The `tcgetattr` call is unconditional in tmux's `client.c` and has been present since control mode was introduced. It appears identically in tmux 2.9, 3.0, 3.4, and 3.6 with no platform guards:

```c
// tmux/client.c (identical across versions 2.9 – 3.6)

/* Set up control mode. */
if (client_flags & CLIENT_CONTROLCONTROL) {
    if (tcgetattr(STDIN_FILENO, &saved_tio) != 0) {
        fprintf(stderr, "tcgetattr failed: %s\n",
            strerror(errno));
        return (1);
    }
    cfmakeraw(&tio);
    // ... configure raw terminal settings ...
    tcsetattr(STDIN_FILENO, TCSANOW, &tio);
}
```

`CLIENT_CONTROLCONTROL` is set when `-CC` is passed. The block runs before any event loop starts. On a plain pipe, `tcgetattr(STDIN_FILENO, …)` returns `-1` with `errno = ENOTTY`, tmux prints the error and returns `1`. There is no `#ifdef` guard, no platform-specific path, no escape hatch.

The only difference between Linux and macOS is the `strerror(ENOTTY)` string:

| Platform | Error string |
|---|---|
| Linux | `Inappropriate ioctl for device` |
| macOS | `Operation not supported by device` |

Both are the same `ENOTTY` errno from the same unconditional `tcgetattr` call.

### Why the man page is misleading

The tmux man page states:

> If the `-CC` flag is used, the client is started in control mode. In control mode, a client accepts commands on standard input and writes output to standard output. **stdin does not need to be a terminal.**

This refers to the tmux *server's* stdin, not the *client process's* stdin. The client process — the one shush spawns — always calls `tcgetattr` on its own stdin and requires a PTY. The distinction between server and client process is subtle but critical.

### Experimental confirmation

```bash
# Plain pipe — fails on both macOS and Linux
tmux -L test-socket -CC attach -t mysession < /dev/null
# → tcgetattr failed: ...

# socat provides a PTY slave — full event stream emitted on both platforms
socat - exec:"tmux -L test-socket -CC attach -t mysession",pty,setsid,ctty
# → \x1bP1000p%begin 1780142087 537 0\r\n
# → %end 1780142087 537 0\r\n
# → %session-changed $0 mysession\r\n
# → %output %0 \033]1337;RemoteHost=...\r\n
```

### Contrast: tmux without `-CC` does NOT require a PTY

One-shot tmux commands (`new-session -d`, `send-keys`, `kill-session`, `list-sessions`, `capture-pane`) work fine with plain pipes or `Stdio::null()`. The source explains why:

```c
// client.c — the tcgetattr block is ONLY entered when CLIENT_CONTROLCONTROL is set
if (client_flags & CLIENT_CONTROLCONTROL) {
    if (tcgetattr(STDIN_FILENO, &saved_tio) != 0) { ... fatal ... }
    // put terminal into raw mode
}
// Falls through for all other commands — no tty check at all
```

On the server side (`server-client.c`), after the client sends its stdin fd over the Unix socket:

```c
if (c->flags & CLIENT_CONTROL)
    control_start(c);          // -CC path: use fd as event I/O channel
else if (c->fd != -1) {
    if (tty_init(&c->tty, c) != 0) {   // tty_init: isatty() check
        close(c->fd);
        c->fd = -1;                     // pipe? fine — just no terminal
    }
    // proceeds headlessly if no terminal found
}
```

`tty_init` calls `isatty(c->fd)` and returns `-1` if the fd is not a tty. The server simply marks the client as having no terminal and continues — no error, no exit. This is how `tmux new-session -d` and all scriptable tmux commands work from CI, cron jobs, and daemons every day.

**Summary:**

| tmux invocation | PTY required? | Why |
|---|---|---|
| `tmux -CC attach` | **Yes** — client process | `tcgetattr(STDIN_FILENO)` unconditional, fatal on `ENOTTY` |
| `tmux new-session -d` | No | `tcgetattr` block skipped; server handles missing tty gracefully |
| `tmux send-keys` | No | Same — one-shot command, no `-CC` flag |
| `tmux capture-pane` | No | Same |
| `tmux kill-session` | No | Same |

This is why the plain-`Command` path works for session lifecycle commands while only the `-CC attach` persistent connection needs the PTY.

### Why the stateless fallback does not work

During investigation, a stateless `TmuxClient` was built that avoided control mode entirely, using per-operation `tmux` invocations (`send-keys`, `capture-pane`, `kill-session`). This approach is insufficient because:

| Capability | Control mode | Stateless (`capture-pane`) |
|---|---|---|
| Detect command completion | Yes — `%end` event after injected marker | No — must poll; unreliable |
| Get exit code | Yes — embedded in marker | No |
| Stream output to browser | Yes — `%output` events are incremental | No — `capture-pane` is a snapshot |
| Marker injection | Yes — `send-keys` over the event channel | Possible but no completion signal |

The marker-based command lifecycle (`IDLE → EXECUTING → IDLE`) depends entirely on the `%output` event stream. Without it, shush cannot know when a command finishes, what its exit code was, or stream live output to the browser.

---

## Decision

Use `portable-pty` (v0.9.0, WezTerm project, MIT) to open a PTY pair and spawn `tmux -CC attach` with the **slave** end as its controlling terminal.

The **master** end is held by shush. Its file descriptor is `dup(2)`'d twice (once for reading events, once for writing commands), set to `O_NONBLOCK`, and wrapped as `tokio::fs::File` for async I/O.

```
┌──────────────────────────────────────────────────────────────────┐
│  shush process                                                    │
│                                                                   │
│  TmuxControlModeClient                                            │
│    stdin  = tokio::fs::File (master fd dup, O_NONBLOCK, writer)  │
│    stdout = BufReader<tokio::fs::File> (master fd dup, reader)   │
│                              │ PTY master                         │
└──────────────────────────────┼───────────────────────────────────┘
                               │ (kernel PTY pair)
                               │ PTY slave = child's stdin/stdout/controlling tty
┌──────────────────────────────┼───────────────────────────────────┐
│  tmux -CC attach process     │                                    │
│    stdin  ──────────────── slave fd   (reads commands from shush)│
│    stdout ──────────────── slave fd   (writes %events to shush)  │
│                                                                   │
│    tcgetattr(slave fd) → succeeds; tmux initialises normally     │
└──────────────────────────────────────────────────────────────────┘
```

`spawn_with_socket` is fully self-contained: it first runs `tmux new-session -d -s <name>` to create the backing session, then attaches in control mode.

### Why `portable-pty` rather than raw `posix_openpt`

`portable-pty` handles the full POSIX PTY setup sequence (`posix_openpt` → `grantpt` → `unlockpt` → `ptsname` → `open(slave)`) plus slave fd inheritance into the child process and `O_CLOEXEC` hygiene. Rolling this manually is error-prone. The crate is maintained as part of WezTerm (a production terminal emulator) and is well-tested on macOS, Linux, and Windows.

### Why `tokio::fs::File` rather than `SyncIoBridge`

`portable-pty`'s `try_clone_reader()` and `take_writer()` return `Box<dyn std::io::Read/Write + Send>` — synchronous blocking types. `tokio_util::io::SyncIoBridge` wraps an *async* type into a sync one (the opposite direction). There is no built-in sync→async bridge in `tokio-util` for general `Read`/`Write`.

On Unix, the PTY master is a file descriptor. `dup(2)` + `fcntl(O_NONBLOCK)` + `tokio::fs::File::from_raw_fd` gives a proper async file that tokio's reactor can poll without blocking the thread pool. This is cheaper than `spawn_blocking` per read and avoids an extra dep.

### Keeping unit tests independent of the PTY path

`TmuxControlModeClient` stores I/O as `Box<dyn AsyncWrite + Unpin + Send>` / `BufReader<Box<dyn AsyncRead + Unpin + Send>>`. Unit tests construct the struct directly with `tokio::io::duplex` streams, exercising `send_keys` and `read_line` without ever touching the PTY or spawning a process. Only the four `#[ignore]` integration tests hit real tmux.

---

## Consequences

**Positive**
- `tmux -CC attach` works correctly on both macOS and Linux with shush running as a daemon (no controlling terminal of its own).
- `TmuxControlModeClient` is fully self-contained: one `spawn_with_socket` call creates and connects to the session.
- Protocol unit tests remain fast and hermetic (no tmux binary required).

**Negative / watch points**
- `libc` and `portable-pty` added as production dependencies.
- The PTY-based fd setup (`dup` + `fcntl(O_NONBLOCK)` + `from_raw_fd`) is Unix-only. If shush ever targets Windows, `spawn_with_socket` needs a `#[cfg(unix)]` / `#[cfg(windows)]` split. (`portable-pty` has a Windows ConPTY backend, so the PTY pair itself is available; only the raw-fd wrapping would differ.)
- tmux emits `\r\n` line endings over the PTY (it believes it is talking to a real terminal and applies `ONLCR` output processing). `read_line` already strips trailing whitespace with `trim_end()`. Future parsers of event lines must not assume bare `\n`.
- The PTY is opened at the default terminal size (80×24). This does not affect control-mode event parsing, but `%output` bytes may contain cursor-position escape sequences calibrated to 80 columns. The browser terminal (xterm.js) renders at its own size; control-mode output is parsed for markers only, not rendered directly.

---

## Alternatives Considered

### A — Plain pipes (`Stdio::piped()`)

Rejected. Fails on all platforms (macOS and Linux) with `tcgetattr failed: ENOTTY`. The check is unconditional in tmux source with no workaround.

### B — `socat` shim

Inject `socat … exec:"tmux …",pty,setsid,ctty` as an intermediate process to provide a PTY. Rejected: adds a hard runtime dependency on `socat`, spawns two extra processes per session, adds latency, and does not solve the problem more cleanly than `portable-pty` does inside shush itself.

### C — Stateless one-shot commands (`send-keys` + `capture-pane` per operation)

Rejected. Cannot detect command completion, cannot stream incremental output, cannot implement the marker-based `IDLE → EXECUTING → IDLE` lifecycle. See capability table in Problem section.

### D — `tmux -CC new-session` (combined create + attach)

Attempted. Fails for the same reason — `tcgetattr` is called unconditionally regardless of the subcommand (`attach` vs `new-session`). Even with a PTY, combining session creation with `-CC` in one invocation ties the server process lifecycle to the control-mode connection; there is no clean way to detach afterward. Keeping them separate (`new-session -d` then `-CC attach`) matches the design spec and is cleaner.

---

## References

- `crates/shush-bin/src/tmux_control.rs` — implementation
- `docs/shush-v01-plan.md` — Phase 3 spec (lines 241–267)
- tmux `client.c` line 344–361: <https://github.com/tmux/tmux/blob/3.6/client.c#L344>
- `portable-pty` crate: <https://crates.io/crates/portable-pty> (WezTerm project)
- tmux control mode: `man tmux`, search for `-CC`
