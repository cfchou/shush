import "./monitor.css";
import { getSession, listSessions } from "../api";
import type { Session } from "../types";
import {
  decodeBase64Bytes,
  normalizeLineEndings,
  reconnectDelayMs,
} from "./stream_utils";
import { TerminalView } from "./terminal";

type StreamMessage =
  | { type: "snapshot"; data: string }
  | { type: "terminal"; data: string }
  | { type: string; data?: string };

interface CommandCardScaffold {
  id: string;
  title: string;
  subtitle: string;
  details: string;
  expanded: boolean;
}

interface MonitorState {
  activeSessionId?: string;
  sessions: Session[];
  selectedSession: Session | null;
  leftCollapsed: boolean;
  rightCollapsed: boolean;
  leftPinnedOpen: boolean;
  rightPinnedOpen: boolean;
  yoloEnabled: boolean;
  commandCards: CommandCardScaffold[];
}

const AUTO_HIDE_LEFT_PX = 1700;
const AUTO_HIDE_RIGHT_PX = 1360;

export function renderMonitor(root: HTMLElement, sessionId?: string): void {
  const state: MonitorState = {
    activeSessionId: sessionId,
    sessions: [],
    selectedSession: null,
    leftCollapsed: false,
    rightCollapsed: false,
    leftPinnedOpen: false,
    rightPinnedOpen: false,
    yoloEnabled: false,
    commandCards: [],
  };

  root.innerHTML = `
    <div class="monitor-page">
      <div class="monitor-shell" id="monitor-shell">
        <aside class="monitor-sidebar monitor-sidebar-left">
          <div class="monitor-sidebar-left-header">
            <div class="monitor-brand">
              <svg
                width="24"
                height="24"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="M12 2L2 7l10 5 10-5-10-5z"></path>
                <path d="M2 17l10 5 10-5"></path>
                <path d="M2 12l10 5 10-5"></path>
              </svg>
              shush v0.1
            </div>
          </div>
          <section class="monitor-sessions-area">
            <div class="monitor-section-title">Active Sessions</div>
            <div class="monitor-session-list" id="monitor-session-list"></div>
          </section>
        </aside>

        <main class="monitor-center-column">
          <header class="monitor-header">
            <div class="monitor-header-left">
              <button
                class="monitor-icon-btn monitor-header-toggle"
                id="toggle-left"
                type="button"
                aria-label="Toggle sessions sidebar"
                title="Toggle sessions sidebar"
              >
                <svg
                  viewBox="0 0 24 24"
                  width="20"
                  height="20"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2"></rect>
                  <line x1="9" y1="3" x2="9" y2="21"></line>
                </svg>
              </button>

              <button
                class="monitor-icon-btn monitor-header-toggle"
                id="toggle-right"
                type="button"
                aria-label="Toggle command sidebar"
                title="Toggle command sidebar"
              >
                <svg
                  viewBox="0 0 24 24"
                  width="20"
                  height="20"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  aria-hidden="true"
                >
                  <rect x="3" y="3" width="18" height="18" rx="2"></rect>
                  <line x1="15" y1="3" x2="15" y2="21"></line>
                </svg>
              </button>

              <div>
                <div class="monitor-badge">Workspace / App</div>
                <a class="monitor-back-link" href="/">Back to dashboard</a>
                <h1 id="monitor-title" class="monitor-title"></h1>
                <p class="monitor-caption" id="monitor-caption">
                  <span id="monitor-left-badge-banner" class="monitor-hidden-banner monitor-left-badge-banner"
                    >Left sidebar hidden to preserve terminal space.</span
                  >
                  <span id="monitor-right-badge-banner" class="monitor-hidden-banner monitor-right-badge-banner"
                    >Right sidebar hidden to preserve terminal space.</span
                  >
                  <span id="monitor-state-caption" class="monitor-state-text"></span>
                </p>
              </div>
            </div>

            <div class="monitor-header-controls">
              <button class="monitor-yolo-control" id="yolo-control" type="button">
                <span class="monitor-text-micro">Safety</span>
                <span class="monitor-yolo-label" id="yolo-label">YOLO Mode Off</span>
                <span class="monitor-yolo-switch" id="yolo-switch" aria-hidden="true"></span>
              </button>
              <button class="monitor-stop-btn" id="stop-btn" type="button">Stop Agent</button>
            </div>
          </header>

          <section class="monitor-center-stage">
            <div class="monitor-center-stage-inner">
              <div class="monitor-terminal-shell">
                <header class="monitor-terminal-header">
                  <span class="monitor-terminal-title" id="terminal-title">bash - shush-agent</span>
                  <span id="terminal-state" class="monitor-terminal-state">Idle</span>
                </header>

                <div class="monitor-terminal-body">
                  <div id="terminal-root" class="monitor-terminal-root"></div>
                  <div id="terminal-placeholder" class="monitor-terminal-placeholder">
                    <div class="monitor-terminal-idle">
                      <div class="monitor-idle-mark" aria-hidden="true">◯</div>
                      <h2 class="monitor-idle-title">Terminal placeholder</h2>
                      <p class="monitor-idle-text">
                        The terminal stays fixed at 1024×768. Select a session to populate this
                        surface with live agent output.
                      </p>
                    </div>
                  </div>
                </div>

                <p id="monitor-status" class="monitor-status">Connecting...</p>
              </div>
            </div>
          </section>
        </main>

        <aside class="monitor-sidebar monitor-sidebar-right">
          <header class="monitor-right-header">
            <div class="monitor-section-title">Pending Commands</div>
            <p class="monitor-visibility-note">Future command cards appear here.</p>
          </header>
          <div class="monitor-right-body">
            <div class="monitor-command-empty" id="monitor-command-empty">
              <h3>No command cards yet</h3>
              <p>This sidebar stays empty until a session is selected and a command is approved.</p>
              <p class="monitor-command-empty-note">Stacked command cards will render here.</p>
            </div>
            <div class="monitor-command-list" id="command-list" aria-live="polite"></div>
          </div>
        </aside>
      </div>
      <p id="session-id" class="session-id">No session selected</p>
    </div>
  `;

  const queryRequired = <T extends Element>(selector: string): T => {
    const node = root.querySelector<T>(selector);
    if (!node) {
      throw new Error(`Missing required monitor UI node: ${selector}`);
    }
    return node;
  };

  const shell = queryRequired<HTMLElement>("#monitor-shell");
  const termRoot = queryRequired<HTMLElement>("#terminal-root");
  const termPlaceholder = queryRequired<HTMLElement>("#terminal-placeholder");
  const statusEl = queryRequired<HTMLElement>("#monitor-status");
  const sessionListEl = queryRequired<HTMLElement>("#monitor-session-list");
  const sessionTitle = queryRequired<HTMLElement>("#monitor-title");
  const sessionIdEl = queryRequired<HTMLElement>("#session-id");
  const sessionStateCaption = queryRequired<HTMLElement>(
    "#monitor-state-caption",
  );
  const leftBadgeBanner = queryRequired<HTMLElement>(
    "#monitor-left-badge-banner",
  );
  const rightBadgeBanner = queryRequired<HTMLElement>(
    "#monitor-right-badge-banner",
  );
  const terminalTitleEl = queryRequired<HTMLElement>("#terminal-title");
  const terminalStateEl = queryRequired<HTMLElement>("#terminal-state");
  const yoloControl = queryRequired<HTMLElement>("#yolo-control");
  const yoloLabel = queryRequired<HTMLElement>("#yolo-label");
  const yoloSwitch = queryRequired<HTMLElement>("#yolo-switch");
  const commandList = queryRequired<HTMLElement>("#command-list");
  const commandEmpty = queryRequired<HTMLElement>("#monitor-command-empty");
  const leftToggle = queryRequired<HTMLElement>("#toggle-left");
  const rightToggle = queryRequired<HTMLElement>("#toggle-right");
  const stopBtn = queryRequired<HTMLElement>("#stop-btn");
  const captionBadges = [leftBadgeBanner, rightBadgeBanner] as const;

  function relativeTimeFromNow(value: string): string {
    const valueDate = new Date(value);
    if (Number.isNaN(valueDate.getTime())) return "unknown";
    const seconds = Math.max(
      0,
      Math.floor((Date.now() - valueDate.getTime()) / 1000),
    );
    if (seconds < 60) return "just now";
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
    if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
    return `${Math.floor(seconds / 86400)}d ago`;
  }

  function updateHeaderState(): void {
    const selected = state.selectedSession;
    if (selected) {
      sessionTitle.textContent = selected.name;
      sessionStateCaption.textContent = `${selected.host || "local"} · State ${selected.state}`;
      sessionIdEl.textContent = `Session: ${selected.id}`;
      terminalTitleEl.textContent = `bash - ${selected.name}`;
      terminalStateEl.textContent = selected.state.toUpperCase();
    } else {
      sessionTitle.textContent = "No session selected";
      sessionStateCaption.textContent =
        "Choose a session from the left to load terminal activity and command approvals.";
      sessionIdEl.textContent = "No session selected";
      terminalTitleEl.textContent = "bash - shush-agent";
      terminalStateEl.textContent = "Idle";
    }
    state.yoloEnabled = selected?.yolo ?? state.yoloEnabled;
    renderYoloState();
  }

  function renderYoloState(): void {
    yoloSwitch.classList.toggle("is-on", state.yoloEnabled);
    yoloLabel.textContent = state.yoloEnabled
      ? "YOLO Mode Active"
      : "YOLO Mode Off";
    yoloLabel.classList.toggle("is-on", state.yoloEnabled);
  }

  function setTerminalMode(hasSession: boolean): void {
    termRoot.classList.toggle("is-hidden", !hasSession);
    termPlaceholder.classList.toggle("is-hidden", hasSession);
  }

  function renderSessionList(): void {
    if (state.sessions.length === 0) {
      sessionListEl.innerHTML = `
        <div class="monitor-empty-state">No active sessions yet. Create one from the dashboard.</div>
      `;
      return;
    }

    sessionListEl.innerHTML = state.sessions
      .map((session) => renderSessionItem(session))
      .join("");
  }

  function renderSessionItem(session: Session): string {
    const isActive = state.activeSessionId === session.id;
    const isBusy = session.state === "pending" || session.state === "executing";
    const itemText = `${escapeHtml(session.name)} · ${relativeTimeFromNow(session.created_at)}`;
    return `
      <a
        href="/monitor/${encodeURIComponent(session.id)}"
        class="monitor-session-item ${isActive ? "is-active" : ""}"
        data-session-id="${escapeHtml(session.id)}"
      >
        <div class="monitor-session-item-title">
          <span class="monitor-status-dot ${isBusy ? "is-active" : ""}"></span>
          ${escapeHtml(session.name)}
        </div>
        <div class="monitor-session-meta">${escapeHtml(
          `${session.state} · ${session.host || "local"}`,
        )}</div>
        <div class="monitor-session-meta monitor-session-meta-time">${itemText}</div>
      </a>
    `;
  }

  function renderCommandCards(): void {
    commandList.innerHTML = "";
    if (state.commandCards.length === 0) {
      commandList.classList.remove("has-content");
      commandEmpty.classList.add("is-visible");
      return;
    }

    commandList.classList.add("has-content");
    commandEmpty.classList.remove("is-visible");
    commandList.innerHTML = state.commandCards
      .map((card) => renderCommandCard(card))
      .join("");

    for (const cardEl of commandList.querySelectorAll(
      ".monitor-command-card",
    )) {
      const summary = cardEl.querySelector<HTMLElement>(
        ".monitor-card-summary",
      );
      summary?.addEventListener("click", () => {
        const currentlyExpanded = cardEl.classList.contains("is-expanded");
        for (const other of commandList.querySelectorAll(
          ".monitor-command-card",
        )) {
          other.classList.remove("is-expanded");
        }
        if (!currentlyExpanded) {
          cardEl.classList.add("is-expanded");
        }
      });
    }
  }

  function renderCommandCard(card: CommandCardScaffold): string {
    return `
      <article class="monitor-command-card ${card.expanded ? "is-expanded" : ""}">
        <div class="monitor-card-summary">
          <div class="monitor-command-icon" aria-hidden="true">•</div>
          <div class="monitor-card-copy">
            <div class="monitor-card-title">${escapeHtml(card.title)}</div>
            <div class="monitor-card-subtitle">${escapeHtml(card.subtitle)}</div>
          </div>
          <div class="monitor-card-chevron" aria-hidden="true">⌄</div>
        </div>
        <div class="monitor-card-details">
          <div class="monitor-code-area">${escapeHtml(card.details)}</div>
          <div class="monitor-card-actions">
            <button class="monitor-btn reject-btn" type="button">Reject</button>
            <button class="monitor-btn approve-btn" type="button">Accept</button>
          </div>
        </div>
      </article>
    `;
  }

  function updateShellShell(): void {
    const width = window.innerWidth;
    const shouldHideLeft = width < AUTO_HIDE_LEFT_PX && !state.leftPinnedOpen;
    const shouldHideRight =
      width < AUTO_HIDE_RIGHT_PX && !state.rightPinnedOpen;

    const leftHidden = shouldHideLeft || state.leftCollapsed;
    const rightHidden = shouldHideRight || state.rightCollapsed;

    shell.classList.toggle("is-left-hidden", leftHidden);
    shell.classList.toggle("is-right-hidden", rightHidden);
    shell.dataset.leftHidden = String(leftHidden);
    shell.dataset.rightHidden = String(rightHidden);

    captionBadges[0]!.dataset.visible = String(leftHidden);
    captionBadges[1]!.dataset.visible = String(rightHidden);
    captionBadges[0]!.textContent =
      "Left sidebar hidden to preserve terminal space.";
    captionBadges[1]!.textContent =
      "Right sidebar hidden to preserve terminal space.";
  }

  function toggleLeftSidebar(): void {
    state.leftCollapsed = !state.leftCollapsed;
    state.leftPinnedOpen = !state.leftCollapsed;
    updateShellShell();
  }

  function toggleRightSidebar(): void {
    state.rightCollapsed = !state.rightCollapsed;
    state.rightPinnedOpen = !state.rightCollapsed;
    updateShellShell();
  }

  function syncSidebarFromResize(): void {
    updateShellShell();
  }

  let terminal: TerminalView | null = null;
  let attempt = 0;
  let closed = false;
  let seenSnapshot = false;
  let activeWs: WebSocket | null = null;

  const setStatus = (message: string): void => {
    statusEl.textContent = message;
  };

  const setStreamClosed = (): void => {
    closed = true;
    activeWs?.close();
    activeWs = null;
  };

  const startLiveTerminal = (): void => {
    setTerminalMode(true);
    if (!terminal) {
      terminal = new TerminalView(termRoot);
      const onResize = () => terminal?.fit();
      window.addEventListener("resize", onResize);
    } else {
      terminal.fit();
    }
    connectStream();
  };

  const startIdleTerminal = (): void => {
    setTerminalMode(false);
    setStatus("No session selected");
    if (activeWs) {
      activeWs.close();
      activeWs = null;
    }
    terminal?.clear();
  };

  const connectStream = () => {
    if (!state.activeSessionId || closed) return;

    const proto = window.location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(
      `${proto}://${window.location.host}/api/sessions/${encodeURIComponent(state.activeSessionId)}/stream`,
    );
    activeWs = ws;

    setStatus(
      attempt === 0
        ? "Connecting..."
        : `Reconnecting (attempt ${attempt + 1})...`,
    );

    ws.addEventListener("open", () => {
      attempt = 0;
      setStatus("Connected, waiting for snapshot...");
    });

    ws.addEventListener("message", (event) => {
      if (typeof event.data !== "string") return;
      let parsed: StreamMessage;
      try {
        parsed = JSON.parse(event.data) as StreamMessage;
      } catch {
        return;
      }

      if (!parsed.data || typeof parsed.data !== "string") return;
      const bytes = normalizeLineEndings(decodeBase64Bytes(parsed.data));
      if (parsed.type === "snapshot") {
        terminal?.clear();
        terminal?.write(bytes);
        terminal?.fit();
        seenSnapshot = true;
        setStatus("Live");
      } else if (parsed.type === "terminal") {
        terminal?.write(bytes);
        if (!seenSnapshot) {
          setStatus("Streaming terminal...");
        }
      }
    });

    ws.addEventListener("close", () => {
      if (activeWs === ws) {
        activeWs = null;
      }
      if (closed) return;
      if (!seenSnapshot) {
        setStatus("Unable to open remote monitor stream. Retrying...");
      }
      const delay = reconnectDelayMs(attempt);
      attempt += 1;
      window.setTimeout(connectStream, delay);
    });

    ws.addEventListener("error", () => {
      if (!seenSnapshot) {
        setStatus("Stream error. Retrying...");
      }
      ws.close();
    });
  };

  async function hydrateSessions(): Promise<void> {
    try {
      state.sessions = await listSessions();
      renderSessionList();
    } catch (err) {
      sessionListEl.innerHTML = `<p class="monitor-error">
        Failed to load sessions: ${(err as Error).message}
      </p>`;
    }
  }

  async function hydrateActiveSession(): Promise<void> {
    if (!state.activeSessionId) {
      state.selectedSession = null;
      startIdleTerminal();
      updateHeaderState();
      renderCommandCards();
      return;
    }

    try {
      const session = await getSession(state.activeSessionId);
      state.selectedSession = session;
      updateHeaderState();
      renderSessionList();
      startLiveTerminal();
      renderCommandCards();
      setStatus("Live");
    } catch (err) {
      state.selectedSession = null;
      setStatus(`Session load failed: ${(err as Error).message}`);
      startIdleTerminal();
      updateHeaderState();
      renderCommandCards();
    }
  }

  yoloControl.addEventListener("click", () => {
    state.yoloEnabled = !state.yoloEnabled;
    renderYoloState();
  });

  yoloControl.addEventListener("keydown", (event: KeyboardEvent) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    state.yoloEnabled = !state.yoloEnabled;
    renderYoloState();
  });

  leftToggle.addEventListener("click", toggleLeftSidebar);
  rightToggle.addEventListener("click", toggleRightSidebar);
  window.addEventListener("resize", syncSidebarFromResize);
  stopBtn.addEventListener("click", () => {
    // stop behavior is intentionally non-destructive placeholder for now
    setStatus("Stop requested (UI placeholder)");
  });
  window.addEventListener("beforeunload", () => {
    setStreamClosed();
  });

  if (sessionId) {
    void hydrateSessions().then(() => {
      void hydrateActiveSession();
    });
  } else {
    void hydrateSessions().then(() => {
      updateHeaderState();
      renderCommandCards();
      startIdleTerminal();
      syncSidebarFromResize();
    });
  }

  syncSidebarFromResize();
  updateHeaderState();
  renderCommandCards();
}

function escapeHtml(value: string): string {
  const div = document.createElement("div");
  div.appendChild(document.createTextNode(value));
  return div.innerHTML;
}
