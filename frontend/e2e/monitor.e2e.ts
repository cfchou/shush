import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { access } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";

type Session = { id: string; name: string };
type Backend =
  | { kind: "local"; host: ""; label: string }
  | { kind: "remote"; host: string; sshConfig: string; label: string };

const currentDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(currentDir, "../..");
const frontendDir = path.resolve(currentDir, "..");

class ServerRuntime {
  private proc: ReturnType<typeof spawn> | null = null;

  async buildFrontend(): Promise<void> {
    await runCommand("npm", ["run", "build"], frontendDir);
  }

  async start(): Promise<void> {
    if (this.proc) return;
    this.proc = spawn("cargo", ["run", "--", "server"], {
      cwd: repoRoot,
      env: {
        ...process.env,
        HOME: remoteHomeDir(),
        SHUSH_SSH_CONFIG: remoteSshConfigPath(),
      },
      stdio: "inherit",
    });
    await waitForServerReady();
  }

  async stop(): Promise<void> {
    if (!this.proc) return;
    const proc = this.proc;
    this.proc = null;
    proc.kill("SIGTERM");
    await new Promise<void>((resolve) => {
      proc.once("exit", () => resolve());
      setTimeout(() => {
        proc.kill("SIGKILL");
        resolve();
      }, 5000);
    });
  }

  async restart(): Promise<void> {
    await this.stop();
    await this.start();
  }
}

const runtime = new ServerRuntime();

test.beforeAll(async () => {
  await runtime.buildFrontend();
  await runtime.start();
});

test.afterAll(async () => {
  await runtime.stop();
});

test("local monitor deep-link and lifecycle", async ({ browser, page }) => {
  const pageErrors: string[] = [];
  page.on("pageerror", (err) => pageErrors.push(String(err)));
  const backend: Backend = { kind: "local", host: "", label: "local" };
  const session = await createSession(backend);
  try {
    const seed = `snapshot-seed-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, seed);

    await installWsProbe(page);
    await page.goto(`/monitor/${encodeURIComponent(session.id)}`);
    await expect(page.locator(".monitor-page")).toBeVisible();
    await expect(page.locator(".session-id")).toContainText(session.id);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => wsEventsLength(page)).toBeGreaterThan(0);
      await expect.poll(() => streamContains(page, seed)).toBe(true);
    }

    const live = `live-update-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, live);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(page, live)).toBe(true);
    }

    const second = await browser.newPage();
    await installWsProbe(second);
    await second.goto(`/monitor/${encodeURIComponent(session.id)}`);
    const shared = `two-tabs-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, shared);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(page, shared)).toBe(true);
      await expect.poll(() => streamContains(second, shared)).toBe(true);
    }
    await second.close();

    const reopened = await browser.newPage();
    await installWsProbe(reopened);
    await reopened.goto(`/monitor/${encodeURIComponent(session.id)}`);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(reopened, shared)).toBe(true);
    }

    await runtime.restart();
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(reopened, shared), {
        timeout: 35_000,
      }).toBe(true);
    }

    const after = `post-restart-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, after);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(reopened, after), {
        timeout: 35_000,
      }).toBe(true);
    }

    await reopened.getByRole("link", { name: "Back to dashboard" }).click();
    await expect(reopened).toHaveURL(/\/$/);
    await expect(reopened.locator(".dashboard")).toBeVisible();
    await reopened.close();

    expect(pageErrors).toEqual([]);
  } finally {
    await deleteSession(session.id);
  }
});

test.describe("remote monitor coverage", () => {
  test.skip(
    !isRemoteEnabled(),
    "Enable remote monitor E2E with SHUSH_E2E_REMOTE=1",
  );

  test("remote monitor deep-link and reconnect snapshot", async ({ page }) => {
    const sshConfig = remoteSshConfigPath();
    await access(sshConfig);
  const backend: Backend = {
      kind: "remote",
      host: process.env.SHUSH_E2E_REMOTE_HOST ?? "shush-docker",
      sshConfig,
      label: "remote",
    };
    const session = await createSession(backend);
  try {
    const seed = `remote-seed-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, seed);

    await installWsProbe(page);
    await page.goto(`/monitor/${encodeURIComponent(session.id)}`);
    await expect(page.locator(".monitor-page")).toBeVisible();
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(page, seed)).toBe(true);
    }

    const live = `remote-live-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, live);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(page, live)).toBe(true);
    }

    await runtime.restart();
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(page, live), {
        timeout: 35_000,
      }).toBe(true);
    }
    } finally {
      await deleteSession(session.id);
    }
  });
});

async function waitForServerReady(timeoutMs = 30_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch("http://127.0.0.1:8100/api/sessions");
      if (response.ok) return;
    } catch {
      // retry
    }
    await sleep(250);
  }
  throw new Error("server did not become ready in time");
}

async function createSession(backend: Backend): Promise<Session> {
  const suffix = randomUUID().slice(0, 8);
  const response = await fetch("http://127.0.0.1:8100/api/sessions", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      name: `monitor-e2e-${backend.label}-${suffix}`,
      host: backend.host,
    }),
  });
  if (!response.ok) {
    throw new Error(`failed to create ${backend.label} session: ${response.status}`);
  }
  const session = (await response.json()) as Session;
  await waitForTmuxSession(backend, session.name);
  return session;
}

async function deleteSession(sessionId: string): Promise<void> {
  await fetch(`http://127.0.0.1:8100/api/sessions/${sessionId}`, { method: "DELETE" });
}

async function sendVisibleLine(backend: Backend, sessionName: string, line: string): Promise<void> {
  const payload = shellQuote(`echo ${line}`);
  const cmd = `tmux -L shush send-keys -t ${sessionName} ${payload} Enter`;
  if (backend.kind === "local") {
    await runCommand("bash", ["-lc", cmd], repoRoot);
  } else {
    await runCommand("ssh", ["-F", backend.sshConfig, backend.host, cmd], repoRoot);
  }

  await waitForCapturedLine(backend, sessionName, line);
}

async function waitForTmuxSession(
  backend: Backend,
  sessionName: string,
): Promise<void> {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    try {
      const probe = `tmux -L shush has-session -t ${sessionName}`;
      if (backend.kind === "local") {
        await runCommand("bash", ["-lc", probe], repoRoot);
      } else {
        await runCommand("ssh", ["-F", backend.sshConfig, backend.host, probe], repoRoot);
      }
      return;
    } catch {
      await sleep(200);
    }
  }
  throw new Error(`tmux session not ready: ${sessionName}`);
}

async function waitForCapturedLine(
  backend: Backend,
  sessionName: string,
  line: string,
): Promise<void> {
  const deadline = Date.now() + 10_000;
  while (Date.now() < deadline) {
    const out = await capturePane(backend, sessionName);
    if (out.includes(line)) {
      return;
    }
    await sleep(200);
  }
  throw new Error(`line did not appear in capture-pane: ${line}`);
}

async function capturePane(backend: Backend, sessionName: string): Promise<string> {
  const args = ["-L", "shush", "capture-pane", "-p", "-t", sessionName];
  if (backend.kind === "local") {
    return await runCommandOutput("tmux", args, repoRoot);
  }
  return await runCommandOutput(
    "ssh",
    ["-F", backend.sshConfig, backend.host, "tmux", ...args],
    repoRoot,
  );
}

function shellQuote(input: string): string {
  return `'${input.replaceAll("'", "'\\''")}'`;
}

async function installWsProbe(page: import("@playwright/test").Page): Promise<void> {
  await page.addInitScript(() => {
    const original = window.WebSocket;
    const events: string[] = [];
    (window as unknown as { __shushWsEvents: string[] }).__shushWsEvents = events;

    class ProbeWebSocket extends original {
      constructor(url: string | URL, protocols?: string | string[]) {
        if (protocols !== undefined) {
          super(url, protocols);
        } else {
          super(url);
        }
        this.addEventListener("message", (event) => {
          if (typeof event.data === "string") {
            events.push(event.data);
          }
        });
      }
    }

    window.WebSocket = ProbeWebSocket;
  });
}

async function wsEventsLength(page: import("@playwright/test").Page): Promise<number> {
  return await page.evaluate(
    () =>
      (window as unknown as { __shushWsEvents?: string[] }).__shushWsEvents
        ?.length ?? 0,
  );
}

async function streamContains(
  page: import("@playwright/test").Page,
  needle: string,
): Promise<boolean> {
  return await page.evaluate((target) => {
    const events =
      (window as unknown as { __shushWsEvents?: string[] }).__shushWsEvents ?? [];
    return events.some((raw) => {
      try {
        const parsed = JSON.parse(raw) as { data?: string };
        if (typeof parsed.data !== "string") return false;
        const decoded = atob(parsed.data);
        return decoded.includes(target);
      } catch {
        return false;
      }
    });
  }, needle);
}

function isRemoteEnabled(): boolean {
  return process.env.SHUSH_E2E_REMOTE === "1";
}

function isStreamAssertEnabled(): boolean {
  return process.env.SHUSH_E2E_ASSERT_STREAM === "1";
}

function remoteHomeDir(): string {
  return process.env.SHUSH_E2E_REMOTE_HOME ?? path.join(repoRoot, ".remote-ssh-home");
}

function remoteSshConfigPath(): string {
  return process.env.SHUSH_SSH_CONFIG ?? path.join(remoteHomeDir(), ".ssh/config");
}

async function runCommand(command: string, args: string[], cwd: string): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: process.env,
      stdio: "inherit",
    });
    child.once("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`command failed: ${command} ${args.join(" ")}`));
      }
    });
    child.once("error", reject);
  });
}

async function runCommandOutput(
  command: string,
  args: string[],
  cwd: string,
): Promise<string> {
  return await new Promise<string>((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.once("exit", (code) => {
      if (code === 0) {
        resolve(stdout);
      } else {
        reject(
          new Error(
            `command failed: ${command} ${args.join(" ")}\n${stderr}`,
          ),
        );
      }
    });
    child.once("error", reject);
  });
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
