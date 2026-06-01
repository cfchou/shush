import "./monitor.css";
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

export function renderMonitor(root: HTMLElement, sessionId: string): void {
  root.innerHTML = `
    <div class="monitor-page">
      <header class="monitor-header">
        <a href="/" class="back-link">Back to dashboard</a>
        <div class="session-id">Session: ${escapeHtml(sessionId)}</div>
      </header>
      <div id="monitor-status" class="monitor-status">Connecting...</div>
      <main class="terminal-shell">
        <div id="terminal-root" class="terminal-root"></div>
      </main>
    </div>
  `;

  const termRoot = root.querySelector<HTMLElement>("#terminal-root");
  const statusEl = root.querySelector<HTMLElement>("#monitor-status");
  if (!termRoot) {
    throw new Error("missing terminal root");
  }
  if (!statusEl) {
    throw new Error("missing monitor status");
  }

  const terminal = new TerminalView(termRoot);
  window.addEventListener("resize", () => terminal.fit());

  let attempt = 0;
  let closed = false;
  let seenSnapshot = false;
  let activeWs: WebSocket | null = null;

  const setStatus = (message: string) => {
    statusEl.textContent = message;
  };

  const connect = () => {
    if (closed) return;

    const proto = window.location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(
      `${proto}://${window.location.host}/api/sessions/${encodeURIComponent(sessionId)}/stream`,
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
        terminal.clear();
        terminal.write(bytes);
        terminal.fit();
        seenSnapshot = true;
        setStatus("Live");
      } else if (parsed.type === "terminal") {
        terminal.write(bytes);
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
      window.setTimeout(connect, delay);
    });

    ws.addEventListener("error", () => {
      if (!seenSnapshot) {
        setStatus("Stream error. Retrying...");
      }
      ws.close();
    });
  };

  connect();

  window.addEventListener("beforeunload", () => {
    closed = true;
    activeWs?.close();
    activeWs = null;
  });
}

function escapeHtml(value: string): string {
  const div = document.createElement("div");
  div.appendChild(document.createTextNode(value));
  return div.innerHTML;
}
