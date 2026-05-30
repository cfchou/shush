# ADR-0003: How tmux Encodes Incremental Screen Changes in Its Output Stream

**Status:** Accepted  
**Date:** 2026-05-30  
**Deciders:** engineering  

---

## Terminal size and coordinate conventions

Two conventions are in use throughout this document and the codebase. Both are correct within their own domain; mixing them up causes off-by-one bugs and misread configs.

### Terminal size: cols × rows (width × height)

```
tmux new-session -x 80 -y 24    →  80 columns wide,  24 rows tall
tmux new-session -x 40 -y 10    →  40 columns wide,  10 rows tall
tmux new-session -x 220 -y 50   →  220 columns wide, 50 rows tall  ← shush default
```

`-x` is **columns (horizontal, width)**, `-y` is **rows (vertical, height)**. The common shorthand `80×24` means 80 cols × 24 rows — width first, height second, matching mathematical (x, y) order.

### Cursor position in ANSI sequences: row ; col (y ; x)

```
ESC[1;1H    →  row 1,  col 1   (top-left corner)
ESC[15;10H  →  row 15, col 10
ESC[H       →  row 1,  col 1   (shorthand, same as ESC[1;1H)
```

`CSI row ; col H` — **row first, column second** — the opposite of the size convention. This is the ANSI standard (ISO 6429); it predates x/y coordinate conventions and was defined in terms of "line;column" on a teletype.

### Why this matters for shush

When the plan says `"tmux session (40×10)"` it means **40 columns wide, 10 rows tall** — not 40 rows. When a progress bar is placed at `ESC[15;10H` it is at **row 15, column 10** — not column 15. Getting these backwards produces bars drawn in the wrong place or sessions opened at the wrong size.

---

## Context

shush streams the output of `tmux attach -t <session> -r` to xterm.js via WebSocket. To reason about bandwidth, correctness, and what xterm.js will receive, we need to understand precisely what tmux puts in that byte stream when the terminal screen changes — for example when a progress bar in the top-left corner advances one step.

---

## Finding: tmux emits cursor-addressed ANSI sequences, not full-screen repaints

tmux does **not** diff the screen and send only the changed cells. It does not send pixel deltas or structured change records. It emits **ANSI/VT escape sequences** that describe how to move the cursor and what to write — exactly the same bytes the application originally wrote to the pty, plus tmux's own bookkeeping sequences around them.

### Verified with a live experiment

A minimal script ran inside a tmux session (40×10), cycling a progress bar through 6 states:

```
[          ]   →   [##        ]   →   [####      ]   →   ...   →   [##########]
```

Each update used: `printf '\033[1;1H[%-10s]' "$bar"` — cursor to row 1 col 1, then overwrite the line.

The raw ANSI stream captured from `tmux attach -r` contained exactly:

```
ESC[H[          ]     ← cursor home, write state 0
ESC[H[##        ]     ← cursor home, overwrite with state 1
ESC[H[####      ]     ← cursor home, overwrite with state 2
ESC[H[######    ]
ESC[H[########  ]
ESC[H[##########]
```

`ESC[H` is `CSI H` — cursor to row 1, col 1 (home). tmux passes through the application's own cursor-positioning and overwrites only what was explicitly written. The rest of the screen is untouched in the stream between updates.

### What appears between two bar updates

Between state 1 (`[##        ]`) and state 2 (`[####      ]`) the stream contained ~200–400 bytes of tmux's own housekeeping — **none of it touching row 1**:

```
ESC[?25l               ← hide cursor (tmux housekeeping)
ESC[?2026h             ← synchronized output mode on
ESC[31m \r ESC[A ...   ← status bar redrawn using relative movement
                          (\r = carriage return to col 1, ESC[A = cursor up 1)
ESC[?2026l             ← synchronized output mode off
ESC[H[####      ]      ← application's next bar state (cursor home + content)
```

**Important:** the status bar is redrawn using **relative cursor movement** (`\r`, `ESC[A`), not an absolute `ESC[row;colH` sequence. tmux knows where the status bar is internally and moves to it relatively. The `ESC[24;1H` form that might appear in other terminal output (and appeared in an earlier draft of this document based on a mis-sized experiment) is **not** how tmux redraws its own status bar. The earlier experiment that showed `ESC[24;1H` was from a session whose actual size was 24 rows (the socat PTY's default), not a 40×10 session — a consequence of the session being resized when an incorrectly-sized client attached. In a correctly-sized 40×10 session, the status bar is at row 10, and it is reached via relative movement, not `ESC[24;1H`.

The application content at row 1 is only touched when the application itself writes to it.

### The first attach: full repaint

When a client first attaches, tmux emits a complete repaint of the current viewport:

```
ESC[?1049h    ← switch to alternate screen buffer
ESC[H         ← cursor home
ESC[2J        ← clear entire screen
ESC[1;1H      ← cursor to (1,1)
... full contents of every non-empty cell, row by row ...
```

This is the "snapshot" that makes a late-joining browser user see the current state. After this initial burst, only incremental updates follow.

---

## Implications for shush

### 1. tmux passes through application sequences, it does not transcode them

The bytes xterm.js receives are effectively the bytes the application wrote to the pty, wrapped in tmux cursor positioning. xterm.js handles these natively — `ESC[H`, `ESC[1;1H`, `ESC[2J`, SGR colour codes, etc. are all standard VT/ANSI sequences every terminal emulator understands.

### 2. Incremental updates are small

A progress bar step that changes 2 characters at a known position costs approximately:

```
ESC[H            3 bytes   (cursor home)
[####      ]    13 bytes   (content — 12 chars + brackets)
──────────────────────────
                16 bytes per update
```

Plus tmux's own housekeeping (status bar redraws via relative cursor movement, synchronized output mode toggles): ~200–400 bytes per clock tick. Bandwidth is negligible for human-facing output.

### 3. Synchronized output mode (`ESC[?2026h/l`)

tmux wraps its own internal screen updates in **Synchronized Output Mode** (DEC private mode 2026). This tells the terminal renderer to buffer output and apply it atomically, preventing tearing. xterm.js supports this mode (see ADR-0002). The application's own writes arrive inside a `?2026h` … `?2026l` pair when tmux decides to flush.

### 4. The FE connection does not need to understand the protocol

SS treats `tmux attach -r` stdout as an opaque byte stream. It reads chunks, base64-encodes them, and sends them to the browser. xterm.js does the protocol interpretation. SS never parses ANSI sequences on the FE path — that would duplicate the terminal emulator.

### 5. Contrast with the SC (control-mode) connection

The SC connection (`-CC`) emits a completely different format:

```
%output %0 \033[1;1H[####      ]\r\n
```

The `%output` event carries the raw bytes **octal-escaped** as a control-mode protocol line. SS parses these events to detect APC markers. xterm.js never sees the `%output` wrapper — only the FE connection's raw bytes reach the browser.

---

## Summary table

| | FE connection (`attach -r`) | SC connection (`-CC attach`) |
|---|---|---|
| **Format** | Raw ANSI byte stream | Control-mode event lines (`%output`, `%begin`, `%end`, …) |
| **Consumer** | xterm.js (browser) | SS event parser (marker detection) |
| **Progress bar update** | `ESC[H[####      ]` + status bar overhead | `%output %0 \033[H[####      ]\r\n` |
| **First attach** | Full repaint (`ESC[2J` + all cells) | `%session-changed` + initial `%output` burst |
| **PTY required** | No (plain pipe) | Yes (see ADR-0001) |

---

## References

- Live experiment: `tmux attach -r` piped via `socat` PTY, bytes decoded with Python `latin-1`
- ADR-0001 — PTY requirement for `-CC` attach
- ADR-0002 — xterm.js rendering model; `terminal.write(Uint8Array)`
- tmux synchronized output: `CSI ? 2026 h/l` (DEC private mode 2026)
- xterm.js `synchronizedOutputMode` docs: <https://xtermjs.org/docs/api/terminal/interfaces/imodes>
