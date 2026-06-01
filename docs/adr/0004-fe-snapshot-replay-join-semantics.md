# ADR-0004: FE Snapshot Is the Screen Source of Truth; Replay Only Bridges the Join Gap

**Status:** Accepted  
**Date:** 2026-06-01  
**Deciders:** engineering  

---

## Context

The browser monitor joins an existing FE stream through the following sequence:

1. SS gets or spawns the per-session `FeMasterHandle`.
2. SS captures the current tmux pane via `capture-pane -p`.
3. SS sends that capture as a WebSocket `{"type":"snapshot"}` message.
4. If the connection is an additional viewer, SS may send a bounded replay of recent FE PTY bytes as normal `{"type":"terminal"}` messages.
5. SS then forwards live `FeEvent::Chunk` broadcasts.

Relevant implementation points:

- `server.rs:120-170` sends `snapshot` first, then optional replay, then enters the live receive loop.
- `fe_master.rs:195-202` appends raw PTY bytes into `replay_buffer` and trims it to `FE_REPLAY_BUFFER_MAX_BYTES`.
- `fe_master.rs:226-238` implements `snapshot()` using `tmux capture-pane -p`.

During review of `FE_REPLAY_BUFFER_MAX_BYTES`, a sizing question arose:

> Should replay capacity be larger than `MONITOR_ROWS * MONITOR_COLS`?

That question assumes the replay buffer is responsible for reconstructing a whole visible screen. The code does not work that way.

---

## Problem

There are two different kinds of state in this join path:

| Mechanism | Data shape | Meaning |
|---|---|---|
| `snapshot()` | tmux pane capture | "What the screen looks like now" |
| `replay_buffer` | raw PTY byte stream | "What bytes were emitted recently" |

These are not interchangeable.

`capture-pane -p` returns a screen-oriented snapshot of pane state at one moment in time. `replay_buffer` stores stream-oriented terminal bytes, including escape sequences, cursor movement, partial writes, and UTF-8 data. A fixed relationship between "screen cells" and "stream bytes" does not exist.

If replay were the primary bootstrap mechanism, SS would need enough recent bytes to deterministically rebuild terminal state from the stream alone. That is a much stronger requirement than the current implementation provides.

---

## Decision

Treat the tmux `snapshot` as the authoritative bootstrap state for new FE viewers.

Treat `replay_buffer` only as a bounded, shared recent-output aid. It improves late-join continuity, but it does **not** currently define an exact per-viewer splice point between snapshot state and subsequent stream bytes.

More specifically:

- The screen base state comes from `snapshot()`, not from replay.
- Replay exists to reduce the observed race window between "snapshot taken" and "viewer fully receiving live broadcast chunks".
- Replay is an implementation-level continuity aid, not a durable scrollback model and not a screen reconstruction mechanism.
- Replay is shared per `FeMasterHandle`; concurrent joiners may consume overlapping replay regions even when their snapshots were taken at different moments.
- `FE_REPLAY_BUFFER_MAX_BYTES` should be sized according to the maximum recent output expected during the join/reconnect window, not according to `MONITOR_ROWS * MONITOR_COLS`.

---

## Join timeline

```
existing tmux session
    │
    │  current visible pane state
    ▼
SS runs capture-pane  ───────────────► snapshot = base screen state
    │
    │  output may continue while join is in progress
    ▼
SS reads replay_buffer ──────────────► optional recent PTY bytes to cover the gap
    │
    ▼
SS subscribes viewer to broadcasts ──► live FeEvent::Chunk stream
```

In practical terms:

- Without `snapshot`, a late-joining viewer may need a full-screen repaint from the byte stream, which the current design does not guarantee.
- Without replay, a late-joining viewer may miss bytes emitted after the snapshot moment but before live streaming is fully established.
- With the current shared replay design, SS reduces that risk best-effort, but does not guarantee that each viewer receives only the exact suffix of bytes after its own snapshot.
- Using both gives a cheap and sufficient v0.1 bootstrap path, with approximate rather than exact snapshot-to-stream splicing.

---

## Consequences

**Positive**

- Clarifies that FE late-join correctness does not depend on replay being "screen-sized".
- Avoids a false sizing rule such as `FE_REPLAY_BUFFER_MAX_BYTES >= MONITOR_ROWS * MONITOR_COLS`.
- Keeps replay policy tied to user-visible continuity during join, which is the actual requirement.

**Negative / watch points**

- Snapshot and replay may overlap, so a short region of output can be duplicated at join time. Accepted for v0.1.
- Because replay is shared per `FeMasterHandle`, concurrent joiners do not have independent replay cutoffs.
- Replay may include bytes that were emitted before a given viewer's snapshot moment, not only after it.
- Replay does not provide a full scrollback guarantee.
- A pathological burst of output during the join window can still exceed the bounded replay capacity; the tradeoff remains best-effort continuity rather than strict losslessness.

### Why overlap is often visually benign

Terminal streams frequently contain operations such as:

- move cursor to a position
- write characters at that position
- set style attributes
- clear a line or region

When replay overlaps with content already reflected in the snapshot, re-applying those operations often redraws the same area and produces the same final visual state. This is the practical reason the current approach can work reasonably well despite imprecise splicing.

However, terminal streams are not universally idempotent. Some sequences depend on current terminal state, including:

- relative cursor movement
- scrolling
- insert/delete line or character operations
- carriage-return/newline interactions
- mode toggles
- chunk-boundary-sensitive escape-sequence parsing

For those cases, overlapped replay can produce duplicate lines, extra cursor movement, transient glitches, or occasionally a different final state. The design therefore relies on overlap being often acceptable in practice, not on a protocol-level guarantee that replaying bytes is always harmless.

### Current concurrency semantics

Under the present code, a joining viewer:

1. takes a snapshot
2. subscribes to broadcasts
3. if `viewer_count() > 1`, reads the entire shared `replay_buffer`
4. receives live broadcast chunks

This means there is no per-viewer replay cursor, offset, sequence number, or timestamp that says "start replaying from here for this snapshot".

If viewers A and B join close together:

- A may snapshot at `t1`
- B may snapshot at `t2`
- both may read the same replay buffer contents at `t3`

That replay buffer represents "recent FE output", not "bytes strictly after my snapshot". The current behavior is therefore approximate by design.

### Current tradeoff

The present design intentionally chooses:

- one shared `FeMasterHandle`
- one shared rolling replay buffer
- low implementation complexity
- best-effort late-join continuity

in exchange for:

- possible snapshot/replay overlap
- approximate rather than exact snapshot-to-stream stitching
- no independent replay boundaries for concurrently joining viewers

---

## Alternatives Considered

### A — Size replay by screen area

Rejected. `MONITOR_ROWS * MONITOR_COLS` counts terminal cells, while replay stores raw stream bytes. The two quantities are not directly comparable.

### B — Use replay as the sole join bootstrap

Rejected. This would require a stronger terminal-state reconstruction guarantee from raw PTY bytes than the current design provides.

### C — Maintain per-chunk replay metadata and per-viewer cutoffs

Deferred. An exact snapshot-to-stream splice would require the replay store to carry sequence numbers or equivalent chunk identities, and the join path to capture a per-viewer cutoff at snapshot time so only the correct suffix is replayed.

Possible shape:

- store replay as chunk records rather than one flat byte vector
- assign each chunk a monotonically increasing sequence number
- record the current sequence boundary when snapshot is taken
- replay only chunks newer than that boundary

This would provide precise per-viewer semantics, including under concurrent joins.

### D — Maintain a server-side terminal model / scrollback buffer

Deferred. This could support stricter late-join recovery semantics, but it adds substantial complexity and duplicates terminal-emulation concerns already handled by tmux + xterm.js.

---

## References

- [`crates/shush-bin/src/server.rs`](../../crates/shush-bin/src/server.rs)
- [`crates/shush-bin/src/fe_master.rs`](../../crates/shush-bin/src/fe_master.rs)
- [`crates/shush-bin/src/session_manager.rs`](../../crates/shush-bin/src/session_manager.rs)
- [docs/fe-replay-join-flow.html](../fe-replay-join-flow.html)
- ADR-0002 — xterm.js transport and late-join snapshot behavior
