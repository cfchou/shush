import "./monitor.css";
import { TerminalView, decodeBase64Bytes } from "./terminal";

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
      <main class="terminal-shell">
        <div id="terminal-root" class="terminal-root"></div>
      </main>
    </div>
  `;

  const termRoot = root.querySelector<HTMLElement>("#terminal-root");
  if (!termRoot) {
    throw new Error("missing terminal root");
  }

  const terminal = new TerminalView(termRoot);
  window.addEventListener("resize", () => terminal.fit());

  let attempt = 0;
  let closed = false;

  const connect = () => {
    if (closed) return;

    const proto = window.location.protocol === "https:" ? "wss" : "ws";
    const ws = new WebSocket(
      `${proto}://${window.location.host}/api/sessions/${encodeURIComponent(sessionId)}/stream`,
    );

    ws.addEventListener("open", () => {
      attempt = 0;
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
      const bytes = decodeBase64Bytes(parsed.data);

      if (parsed.type === "snapshot") {
        terminal.clear();
        terminal.write(bytes);
        terminal.fit();
      } else if (parsed.type === "terminal") {
        terminal.write(bytes);
      }
    });

    ws.addEventListener("close", () => {
      if (closed) return;
      const delay = Math.min(30000, 1000 * 2 ** attempt);
      attempt += 1;
      window.setTimeout(connect, delay);
    });

    ws.addEventListener("error", () => {
      ws.close();
    });
  };

  connect();

  window.addEventListener("beforeunload", () => {
    closed = true;
  });
}

function escapeHtml(value: string): string {
  const div = document.createElement("div");
  div.appendChild(document.createTextNode(value));
  return div.innerHTML;
}
