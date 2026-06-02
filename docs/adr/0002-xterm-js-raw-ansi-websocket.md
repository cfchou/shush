# ADR-0002: xterm.js Receives Raw ANSI Bytes via WebSocket — No Frame Synchronisation Needed

**Status:** Accepted  
**Date:** 2026-05-30  
**Deciders:** engineering  

---

## Context

shush streams live terminal output from a tmux session to a browser-hosted xterm.js terminal. The path is:

```
tmux session
  └─ tmux attach -t <session> -r          # FE connection, read-only, plain pipe
       └─ SS reads stdout chunks           # tokio BufReader
            └─ base64-encode → JSON        # {"type":"terminal","data":"<b64>"}
                 └─ WebSocket frame        # Axum ws_handler → browser
                      └─ terminal.write()  # xterm.js
```

The question this ADR answers: **what does xterm.js expect as input, and how does it render it?**

---

## Findings

### xterm.js accepts raw ANSI bytes directly

`terminal.write(data)` accepts either a `string` or a `Uint8Array`. When given a `Uint8Array`, bytes are always interpreted as UTF-8. There is no intermediate parsing layer needed on the SS side — tmux's raw ANSI output (cursor moves, colour codes, SGR sequences, etc.) can be fed straight in.

```typescript
// Both forms are valid
terminal.write("hello\r\n");
terminal.write(new Uint8Array([0x1b, 0x5b, 0x32, 0x4a]));  // ESC[2J clear screen
```

SS only needs to base64-encode the raw bytes for safe JSON transport. The browser decodes and passes the `Uint8Array` directly to xterm.js.

### Rendering is not frame-by-frame

xterm.js is a **stateful VT parser + renderer**. `terminal.write()` feeds the parser; the parser mutates an internal buffer model; the renderer paints dirty cells to the canvas. There is no "flush" call — the two async boundaries are driven entirely by the browser scheduler.

The pipeline has three stages separated by two async boundaries:

```
terminal.write(bytes)          ← STAGE 1: INPUT — synchronous
  └─ appended to write queue;
     returns immediately

         ↓  setTimeout (async boundary 1)

parser runs on queued bytes     ← STAGE 2: PARSING — async, time-sliced
  └─ ANSI/VT sequences decoded
  └─ cursor moved, cell attributes set
  └─ internal buffer model updated
  └─ onWriteParsed fires (at most once per frame)

         ↓  requestAnimationFrame (async boundary 2)

renderer evaluates dirty rows   ← STAGE 3: RENDER — async, ≤60 fps
  └─ diffs changed cells in buffer model
  └─ paints to canvas
  └─ onRender fires
```

xterm.js has four distinct execution contexts (from the official docs):

| Phase | Mechanism | What happens |
|---|---|---|
| **Terminal input** | Synchronous | `term.write(chunk)` appends to write queue; returns immediately |
| **Input processing** | Async (`setTimeout`) | Bytes decoded, ANSI sequences parsed, buffer model mutated |
| **Screen update** | `requestAnimationFrame` | Renderer diffs dirty rows and repaints canvas; ≤60 fps |
| **Event processing** | Browser event loop | `onData`, `onWriteParsed`, `onRender` etc. fired between the above |

Key properties:

- **`term.write()` is non-blocking.** The screen does not update synchronously. Data is queued.
- **Many writes coalesce into one render.** Under heavy output, xterm.js batches multiple parse cycles before a single canvas paint. There is no per-chunk screen flash.
- **"Flush" is the wrong mental model.** There is no explicit drain call. Both async boundaries are driven by the browser's own scheduler. The caller cannot force an immediate repaint.
- **`onWriteParsed` fires at most once per frame**, after parsing completes for that batch. It may fire while more writes are still pending if input is heavy.
- **A specific chunk is not guaranteed to appear in a specific frame.** The buffer model may advance further before the next repaint.

### FE attach needs a PTY in this environment

The original design assumed `tmux attach -r` (without `-CC`) would work headlessly with piped stdio. On the current runtime target (tmux `3.6b` on macOS), that assumption is false:

```bash
tmux -L shush attach -r -t mysession < /dev/null
# → open terminal failed: not a terminal
```

Running the same command under a PTY wrapper succeeds, and tmux registers a real read-only client visible via `list-clients`. So the FE path still remains a transparent ANSI pipe to xterm.js, but the spawning mechanism must use a PTY-backed tmux client rather than plain `Stdio::piped()`.

---

## Decision

### SS side

- Spawn `tmux -L shush attach -t <session> -r` under a PTY and read from the PTY master.
- Read stdout-equivalent bytes in chunks (no special framing needed — xterm.js handles split escape sequences across chunks).
- Base64-encode each chunk and send as `{"type":"terminal","data":"<b64>"}`.
- On new WebSocket connection, send a `{"type":"snapshot","data":"<b64>"}` first using `tmux capture-pane -p` output, so late-joiners see current state.

### Browser side

```typescript
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.type === 'snapshot') {
    terminal.clear();
    terminal.write(base64ToUint8Array(msg.data));
  } else if (msg.type === 'terminal') {
    terminal.write(base64ToUint8Array(msg.data));
  }
};

function base64ToUint8Array(b64: string): Uint8Array {
  const binary = atob(b64);
  return Uint8Array.from(binary, c => c.charCodeAt(0));
}
```

No frame synchronisation, no ACK, no back-pressure protocol needed for v0.1.

### Implementation addendum (2026-06-01)

During remote monitor hardening (Issue 08), two pragmatic additions were introduced:

1. **Late-join bootstrap replay**
   - On additional viewer join, SS may send a bounded replay of recent FE terminal bytes immediately after `snapshot`.
   - This is still sent as normal `{"type":"terminal","data":"<b64>"}` messages.
   - Purpose: reduce transient blank/incomplete state for viewers joining an already-live FE stream.

2. **Client-side LF normalization**
   - Browser monitor normalizes lone `\n` to `\r\n` before `terminal.write(...)`.
   - Purpose: avoid line-start drift artifacts observed in remote attach/detach workflows.

These are implementation-level mitigations and do not change the core architecture (PTY-backed FE stream, base64 WS transport, xterm.js rendering pipeline).

### Flow control (v0.1: optional, noted for future)

If SS sends data faster than xterm.js can parse, the internal buffer grows unboundedly. The xterm.js docs recommend pausing the source when the `write()` callback has not yet fired:

```typescript
// Flow-controlled write (v0.2+)
function writeWithBackpressure(data: Uint8Array) {
  return new Promise<void>(resolve => terminal.write(data, resolve));
}
```

For v0.1 — a single human watching a terminal — the throughput is nowhere near the threshold where this matters. Deferred.

---

## Consequences

**Positive**
- No translation layer needed between tmux ANSI output and xterm.js input. SS is a transparent pipe.
- Chunked streaming works naturally — xterm.js handles escape sequences split across chunk boundaries.
- Late-join via `capture-pane` snapshot is sufficient for v0.1 (no byte-level scrollback buffer needed in SS).
- The FE connection still delivers raw ANSI bytes unchanged, but on this environment it must be PTY-backed rather than spawned with plain piped stdio.

**Negative / watch points**
- `capture-pane` only captures the visible viewport, not the scrollback buffer. A browser user who connects after output has scrolled off will not see that history. Accepted limitation for v0.1.
- xterm.js renders at the browser's display rate (≤ 60 fps). Under very high output volume, the last rendered frame may skip intermediate states. For the shush use case (command output monitoring) this is acceptable.
- The base64 encoding adds ~33% wire overhead compared to binary WebSocket frames. Acceptable for v0.1; binary frames with `ArrayBuffer` can replace it later if needed.
- No flow control in v0.1. If a command produces gigabytes of output, the WebSocket buffer and xterm.js internal buffer will grow. Mitigated by the fact that shush is loopback-only and the session owner is watching — they would abort before this became a problem.
- Bootstrap replay may duplicate a short region around initial join (snapshot + replay overlap). Accepted for v0.1 in exchange for improved late-join stability.
- LF normalization means browser input is no longer a strict byte-for-byte mirror of transport payload for newline bytes.

---

## Alternatives Considered

### Binary WebSocket frames

Send raw bytes as `ArrayBuffer` instead of base64-in-JSON. Saves ~33% bandwidth and avoids a JS `atob` call. Rejected for v0.1: mixing binary and text frames on one WebSocket connection complicates the message dispatch logic (need to distinguish terminal data from JSON command-card messages by frame type). Revisit in v0.2 if profiling shows the base64 overhead matters.

### Server-side ANSI stripping / parsing

Have SS parse the ANSI stream and send structured cell updates. Rejected: xterm.js is specifically designed to handle raw ANSI; re-implementing a terminal state machine in SS would duplicate it badly and break on any sequence not anticipated.

### One WebSocket connection per message type

Separate WebSocket for terminal bytes (binary), separate one for JSON events (cards, state). Clean separation but doubles connection management complexity. Deferred.

---

## References

- xterm.js `terminal.write()` API: <https://xtermjs.org/docs/api/terminal/classes/terminal>
- xterm.js flow control guide: <https://xtermjs.org/docs/guides/flowcontrol>
- xterm.js hooks / execution contexts: <https://xtermjs.org/docs/guides/hooks>
- `docs/shush-v01-plan.md` — Phase 4 spec (lines 294–305)
- ADR-0001 — FE connection does not need PTY; SC connection does
