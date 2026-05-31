import { listSessions, createSession, deleteSession } from "../api";
import type { Session } from "../types";
import "./dashboard.css";

export function renderDashboard(root: HTMLElement) {
  root.innerHTML = `
    <div class="dashboard">
      <header class="dashboard-header">
        <h1>shush</h1>
      </header>

      <form class="create-form" id="create-form">
        <input
          type="text"
          name="name"
          placeholder="Session name"
          required
          autocomplete="off"
        />
        <input
          type="text"
          name="host"
          placeholder="Host (optional)"
          autocomplete="off"
        />
        <button type="submit">Create Session</button>
      </form>

      <div class="session-list" id="session-list"></div>
    </div>
  `;

  const form = root.querySelector<HTMLFormElement>("#create-form")!;
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const data = new FormData(form);
    const name = data.get("name") as string;
    const host = data.get("host") as string;
    form.reset();
    try {
      await createSession(name, host);
      await refreshList(root);
    } catch (err) {
      console.error("create failed", err);
    }
  });

  refreshList(root);
}

async function refreshList(root: HTMLElement) {
  const list = root.querySelector<HTMLElement>("#session-list")!;
  try {
    const sessions = await listSessions();
    list.innerHTML = sessions.map(renderCard).join("");
  } catch (err) {
    list.innerHTML = `<p class="error">Failed to load sessions: ${err}</p>`;
  }
}

function renderCard(session: Session): string {
  return `
    <div class="session-card" data-id="${session.id}">
      <div class="session-card-body">
        <div class="session-name">
          <a class="monitor-link" href="/monitor/${encodeURIComponent(session.id)}">${escapeHtml(session.name)}</a>
        </div>
        <div class="session-meta">
          <span class="session-state">${session.state}</span>
          <span class="session-host">${escapeHtml(session.host || "local")}</span>
          <span class="session-time">${new Date(session.created_at).toLocaleString()}</span>
        </div>
      </div>
      <button class="delete-btn" data-id="${session.id}">&times;</button>
    </div>
  `;
}

function escapeHtml(s: string): string {
  const div = document.createElement("div");
  div.appendChild(document.createTextNode(s));
  return div.innerHTML;
}

document.addEventListener("click", async (e) => {
  const target = e.target as HTMLElement;
  if (target.classList.contains("delete-btn")) {
    const id = target.getAttribute("data-id")!;
    try {
      await deleteSession(id);
      const card = target.closest(".session-card")!;
      card.remove();
    } catch (err) {
      console.error("delete failed", err);
    }
  }
});
