import { renderDashboard } from "./dashboard/dashboard";
import { renderMonitor } from "./monitor/monitor";

const app = document.getElementById("app");
if (!app) throw new Error("no #app element found");

const path = window.location.pathname;
const monitorMatch = path.match(/^\/monitor\/([^/]+)$/);

if (monitorMatch) {
  renderMonitor(app, decodeURIComponent(monitorMatch[1]));
} else {
  renderDashboard(app);
}
