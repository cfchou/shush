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
        +create(name, host) Result~Uuid~
        +delete(id)
        +get(id) Session
        +list() Vec~Session~
    }

    class CommandQueue {
        +submit(command) CommandCard
        +approve()
        +deny()
        +abort()
        +process_next()
    }

    class TmuxControlModeClient {
        +spawn(session_name) Result~Self~
        +event_stream() impl Stream~TmuxEvent~
        +send_keys(text)
        +inject_command(command)
        +abort()
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
        +End(Nonce)
    }

    Session "1" *-- "0..1" CommandCard : current_command
    SessionManager "1" *-- "*" Session : manages
    CommandQueue "1" *-- "*" CommandCard : queues
    TmuxControlModeClient ..> TmuxEvent : produces
    MarkerInjector ..> Nonce : generates
    MarkerDetector ..> MarkerEvent : detects
    TmuxControlModeClient ..> MarkerInjector : uses
    TmuxControlModeClient ..> MarkerDetector : uses
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
