import { renderDashboard } from "./dashboard/dashboard";

const app = document.getElementById("app");
if (!app) throw new Error("no #app element found");

renderDashboard(app);
