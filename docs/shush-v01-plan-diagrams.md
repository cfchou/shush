# shush v0.1 — Architectural Diagrams

## 1. Class Diagram — Core Data Model

```mermaid
---
title: shush Core Types
---
classDiagram
    class SessionState {
        <<enumeration>>
        Idle
        Pending
        Executing
    }

    class CommandState {
        <<enumeration>>
        Pending
        Executing
        Completed(i32)
        Rejected
        Aborted
    }

    class CommandCard {
        +Uuid id
        +String command
        +CommandState state
        +Option~i32~ exit_code
        +String output
        +DateTime created_at
        +Option~DateTime~ resolved_at
        +Option~String~ resolved_by
    }

    class Session {
        +Uuid id
        +String name
        +String host
        +SessionState state
        +bool yolo
        +Option~CommandCard~ current_command
        +DateTime created_at
    }

    class SessionManager {
        +create(name, host) Session
        +delete(id) bool
        +get(id) Option~Session~
        +list() Vec~Session~
    }

    class TmuxControlModeClient {
        +spawn(session_name, session_host) Result~Self~
        +send_keys(text) Result
        +inject_command(command) Result~Nonce~
        +read_event() Option~TmuxEvent~
        +kill()
    }

    class TmuxEvent {
        <<enumeration>>
        +Begin(u64, u64, u32)
        +End(u64, u64, u32)
        +Error(u64, u64, u32, String)
        +Output(pane, data)
        +WindowAdd(String)
        +SessionChanged(String, u64)
        +Unknown(String)
    }

    class MarkerInjector {
        +new() Self
        +inject(command) (String, Nonce)
    }

    class MarkerDetector {
        +new() Self
        +feed(bytes) Vec~MarkerEvent~
    }

    class Nonce {
        +hex() String
    }

    class MarkerEvent {
        <<enumeration>>
        +Start(Nonce)
        +End(nonce, exit_code)
    }

    class FeEvent {
        <<enumeration>>
        +Chunk(Vec~u8~)
        +Closed
    }

    class FeMasterHandle {
        +session_name: String
        +session_host: String
        +replay_buffer: Vec~u8~
        +viewers: AtomicUsize
        +alive: AtomicBool
        +spawn(name, host) Arc~Self~
        +snapshot() String
        +subscribe() Receiver~FeEvent~
        +queue_command_echo_replacement(command)
        +replay_bytes() Vec~u8~
        +viewer_count() usize
        +is_alive() bool
    }

    class FeMasterRegistry {
        +get_or_spawn(id, name, host) Arc~FeMasterHandle~
        +on_disconnect(id)
        +queue_command_echo_replacement(id, command)
    }

    Session "1" *-- "0..1" CommandCard : current_command
    SessionManager "1" *-- "*" Session : manages
    TmuxControlModeClient ..> TmuxEvent : reads
    TmuxControlModeClient ..> MarkerInjector : wraps commands
    MarkerDetector ..> MarkerEvent : emits
    MarkerInjector ..> Nonce : generates
    FeMasterRegistry "1" *-- "*" FeMasterHandle : manages
    FeMasterHandle ..> FeEvent : broadcasts
```

---

## 2. Sequence Diagram — Command Execution Flow

```mermaid
---
title: Command Submit → Approve → Execute → Complete
---
sequenceDiagram
    actor User
    participant Client as Browser / SC
    participant SS as SS Server
    participant CQ as CommandQueue
    participant TCC as TmuxControl
    participant tmux

    User->>Client: types command "npm test"

    Client->>SS: POST /sessions/:id?action=submit<br>{"command":"npm test"}
    activate SS
    SS->>CQ: submit("npm test")
    activate CQ
    CQ-->>SS: CommandCard (PENDING)
    deactivate CQ
    SS-->>Client: 200 Session {current_command: PENDING}
    deactivate SS

    User->>Client: clicks [Approve]

    Client->>SS: POST /sessions/:id?action=approve
    activate SS
    SS->>CQ: approve()
    activate CQ
    CQ->>TCC: inject_command("npm test")
    activate TCC

    Note over TCC: Wraps command with<br>APC markers + nonce

    TCC->>tmux: send-keys (wrapped command)
    deactivate CQ

    tmux-->>TCC: %output (marker stream + cmd output)
    TCC-->>CQ: MarkerEvent::End(nonce)
    activate CQ
    Note over CQ: CommandCard → COMPLETED
    CQ-->>SS: command complete
    deactivate CQ

    SS-->>Client: 200 Session {state: idle, current_command: completed}
    deactivate SS
    deactivate TCC

    User->>Client: sees exit code + output

    Note over SS,Client: Browser FE stream is rendering-only.<br/>Marker completion comes from control-mode `%output`.<br/>Backend FE filter rewrites wrapped command echo in-place before xterm renders it.
```

---

## 3. Flowchart — Command Lifecycle State Machine

```mermaid
---
title: Command State Machine
---
stateDiagram-v2
    [*] --> Idle : session created

    Idle --> Pending : submit
    Idle --> Executing : submit (YOLO)

    Pending --> Executing : approve
    Pending --> Idle : deny

    Executing --> Idle : complete
    Executing --> Idle : abort

    Idle --> [*] : session deleted
```

---

## 4. Flowchart — Session Lifecycle

```mermaid
---
title: Session Lifecycle
---
flowchart TD
    A[POST /api/sessions] --> B{name + host}
    B --> C[Generate UUID]
    C --> D[Create Session struct<br>state = Idle]
    D --> E[tmux -L shush new-session -d -s &lt;name&gt;]
    E --> F[tmux -L shush -CC attach -t &lt;name&gt;]
    F --> G[Spawn event reader task<br>tokio::spawn]
    G --> H[Store TmuxControlModeClient<br>in SessionManager]
    H --> I[Return Session JSON]

    style A fill:#dae8fc,stroke:#6c8ebf
    style B fill:#fff2cc,stroke:#d6b656
    style C fill:#d5e8d4,stroke:#82b366
    style I fill:#d5e8d4,stroke:#82b366
```

---

## 5. Sequence Diagram — Server ↔ Remote Target Interactions

Covers every SSH/tmux command the shush server issues against a remote host, the flags used, and what each does.

```mermaid
---
title: Server to Remote Target (SSH + tmux commands)
---
sequenceDiagram
    participant SS as SS Server
    participant SSH as ssh
    participant Remote as tmux on remote host

    Note over SS,Remote: Session creation

    SS->>SSH: has-session -t name
    SSH-->>SS: exit 0 (exists) / exit 1 (not found)

    SS->>SSH: new-session -d -s name
    SSH-->>SS: exit 0

    SS->>SSH: set-window-option -t name window-size manual
    SSH-->>SS: exit 0

    SS->>SSH: resize-window -t name -x 220 -y 50
    SSH-->>SS: exit 0

    Note over SS,Remote: FE master - browser terminal stream

    SS->>SSH: attach -f read-only,ignore-size -t name
    SSH-->>SS: raw ANSI bytes (continuous)

    Note over SS,Remote: Snapshot on WS connect

    SS->>SSH: capture-pane -p -t name
    SSH-->>SS: pane text

    Note over SS,Remote: Session deletion

    SS->>SSH: kill-session -t name
    SSH-->>SS: exit 0
```

All one-shot commands use `ssh -o BatchMode=yes [-F config] host tmux -L shush ...`.
The FE master attach uses `ssh -o BatchMode=yes -tt [-F config] host tmux -L shush ...`.

### SSH flags

| Flag | Where used | Purpose |
|---|---|---|
| `-o BatchMode=yes` | all commands | Fail immediately if key auth unavailable — never prompt. The server has no TTY so any SSH prompt would hang forever. |
| `-F config` | all commands | Optional path to SSH config file (from `SHUSH_SSH_CONFIG` env var). Sets host alias, identity file, port, etc. |
| `-tt` | FE master attach only | Force PTY allocation on the remote side. `tmux attach` calls `tcgetattr()` on startup and exits with `ENOTTY` if it has no controlling terminal. |

### tmux flags

| Command / flag | Where used | Purpose |
|---|---|---|
| `-L shush` | all commands | Use a named tmux socket (`shush`) instead of the default. Isolates shush-managed sessions from the user's own tmux. |
| `new-session -d -s name` | session creation | Create a detached (`-d`) session with a given name (`-s`). Detached means no terminal is needed for creation. |
| `set-window-option window-size manual` | session creation | Disable tmux's automatic pane resize when clients attach/detach. Without this, an external SSH attach at a different terminal size resizes the pane and corrupts the browser view. |
| `resize-window -x 220 -y 50` | session creation | Lock the window to exactly the same dimensions xterm.js initialises to. Combined with `window-size manual`, geometry stays stable regardless of external clients. |
| `attach -f read-only,ignore-size` | FE master | `read-only` prevents the FE client from sending keystrokes. `ignore-size` excludes this client from window size negotiation so external attaches cannot resize the pane. |
| `capture-pane -p` | WS connect | Dump the current pane content as plain text to stdout (`-p`). Used as the initial snapshot. Note: omits the tmux status bar; the replay buffer is sent immediately after for additional viewers to restore the full frame. |
| `kill-session -t name` | session deletion | Terminate the named session and all its windows/panes. |

---

## 6. Sequence Diagram — Browser Monitor Lifecycle

Shows how the browser WebSocket interacts with the server's FE master handle across connect, multi-tab, refresh, and EOF recovery scenarios.

```mermaid
---
title: Browser Monitor — WS + FE Lifecycle
---
sequenceDiagram
    participant B1 as Browser Tab 1
    participant B2 as Browser Tab 2
    participant WS as WS Handler (server)
    participant FE as FeMasterHandle
    participant PTY as FE PTY Process (ssh+tmux attach)

    Note over B1,PTY: ── First viewer connects ─────────────────────────

    B1->>WS: GET /api/sessions/:id/stream (WS upgrade)
    WS->>FE: get_or_spawn() — no handle yet, spawn new
    FE->>PTY: ssh -tt ... tmux attach -f read-only,ignore-size
    PTY-->>FE: raw ANSI stream (continuous)
    Note over FE: Reader task starts:<br>fills replay_buffer, broadcasts FeEvent::Chunk
    WS->>PTY: capture-pane (snapshot)
    PTY-->>WS: pane text
    WS-->>B1: {"type":"snapshot","data":"..."}
    Note over B1: xterm.js renders snapshot,<br>then receives live FeEvent::Chunk frames

    Note over B1,PTY: ── Second viewer joins ───────────────────────────

    B2->>WS: GET /api/sessions/:id/stream (WS upgrade)
    WS->>FE: get_or_spawn() — handle alive, reuse
    Note over FE: viewer_count becomes 2
    WS->>PTY: capture-pane (snapshot for B2)
    PTY-->>WS: pane text
    WS-->>B2: {"type":"snapshot","data":"..."}
    Note over WS: is_additional_viewer=true:<br>read replay_buffer and send as terminal frame
    WS-->>B2: {"type":"terminal","data":"<replay>"}
    Note over B2: Full client-rendered frame<br>including tmux status bar

    Note over B1,PTY: ── Browser refresh (tab 1) ──────────────────────

    B1->>WS: beforeunload → WS close()
    Note over FE: viewer_count drops to 1<br>5s idle timer starts for tab 1's slot
    B1->>WS: GET /api/sessions/:id/stream (new WS)
    WS->>FE: get_or_spawn() — handle alive, reuse<br>(timer cancelled by on_connect)
    WS-->>B1: snapshot + (replay if viewer_count > 1)

    Note over B1,PTY: ── FE PTY process exits (EOF) ───────────────────

    PTY-->>FE: EOF
    Note over FE: alive = false<br>broadcast FeEvent::Closed
    FE-->>WS: FeEvent::Closed (all subscribers)
    WS-->>B1: WS close
    WS-->>B2: WS close
    Note over B1,B2: Browser reconnect backoff fires
    B1->>WS: GET /api/sessions/:id/stream (reconnect)
    WS->>FE: get_or_spawn() — is_alive()=false<br>evict stale handle, spawn fresh PTY
    FE->>PTY: ssh -tt ... tmux attach (new process)
    WS-->>B1: fresh snapshot
```
