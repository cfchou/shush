import { expect, test } from "@playwright/test";
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { access } from "node:fs/promises";
import net from "node:net";
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
  private readonly port: number;

  constructor(port: number) {
    this.port = port;
  }

  baseUrl(): string {
    return `http://127.0.0.1:${this.port}`;
  }

  async buildFrontend(): Promise<void> {
    await runCommand("npm", ["run", "build"], frontendDir);
  }

  async start(): Promise<void> {
    if (this.proc) return;
    this.proc = spawn("cargo", ["run", "--", "server", "--port", String(this.port)], {
      cwd: repoRoot,
      env: {
        ...process.env,
        HOME: remoteHomeDir(),
        SHUSH_SSH_CONFIG: remoteSshConfigPath(),
      },
      stdio: "inherit",
    });
    await waitForServerReady(this.baseUrl());
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

let runtime: ServerRuntime;

test.beforeAll(async () => {
  const port = await allocateFreePort();
  runtime = new ServerRuntime(port);
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
  const session = await createSession(runtime.baseUrl(), backend);
  try {
    const seed = `snapshot-seed-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, seed);

    await installWsProbe(page);
    await page.goto(`${runtime.baseUrl()}/monitor/${encodeURIComponent(session.id)}`);
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
    await second.goto(`${runtime.baseUrl()}/monitor/${encodeURIComponent(session.id)}`);
    const shared = `two-tabs-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, shared);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(page, shared)).toBe(true);
      await expect.poll(() => streamContains(second, shared)).toBe(true);
    }
    await second.close();

    const reopened = await browser.newPage();
    await installWsProbe(reopened);
    await reopened.goto(`${runtime.baseUrl()}/monitor/${encodeURIComponent(session.id)}`);
    if (isStreamAssertEnabled()) {
      await expect.poll(() => streamContains(reopened, shared)).toBe(true);
    }

    await runtime.restart();
    const sessionSurvivedRestart = await sessionExists(runtime.baseUrl(), session.id);
    if (isStreamAssertEnabled() && sessionSurvivedRestart) {
      await expect.poll(() => streamContains(reopened, shared), {
        timeout: 35_000,
      }).toBe(true);
    }

    const after = `post-restart-${randomUUID().slice(0, 8)}`;
    await sendVisibleLine(backend, session.name, after);
    if (isStreamAssertEnabled() && sessionSurvivedRestart) {
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
    await deleteSession(runtime.baseUrl(), session.id);
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
    const session = await createSession(runtime.baseUrl(), backend);
    try {
      const seed = `remote-seed-${randomUUID().slice(0, 8)}`;
      await sendVisibleLine(backend, session.name, seed);

      await installWsProbe(page);
      await page.goto(`${runtime.baseUrl()}/monitor/${encodeURIComponent(session.id)}`);
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
      const sessionSurvivedRestart = await sessionExists(runtime.baseUrl(), session.id);
      if (isStreamAssertEnabled() && sessionSurvivedRestart) {
        await expect.poll(() => streamContains(page, live), {
          timeout: 35_000,
        }).toBe(true);
      }
    } finally {
      await deleteSession(runtime.baseUrl(), session.id);
    }
  });

  test("remote approve flow shows command and output without marker garbage", async ({ page }) => {
    const sshConfig = remoteSshConfigPath();
    await access(sshConfig);
    const backend: Backend = {
      kind: "remote",
      host: process.env.SHUSH_E2E_REMOTE_HOST ?? "shush-docker",
      sshConfig,
      label: "remote-approve-output",
    };
    const session = await createSession(runtime.baseUrl(), backend);

    try {
      await installWsProbe(page);
      await page.goto(`${runtime.baseUrl()}/monitor/${encodeURIComponent(session.id)}`);
      await expect(page.locator(".monitor-page")).toBeVisible();

      const before = await terminalText(page);
      await page.screenshot({ path: test.info().outputPath("before-approve-flow.png") });

      await submitCommand(runtime.baseUrl(), session.id, "echo hello");
      await approveCommand(runtime.baseUrl(), session.id);

      await expect
        .poll(async () => await terminalText(page), { timeout: 35_000 })
        .toContain("echo hello");
      await expect
        .poll(async () => await terminalText(page), { timeout: 35_000 })
        .toContain("hello");

      await expect
        .poll(async () => {
          const rows = await terminalRows(page);
          const commandRow = rows.findIndex((row) => row.includes("echo hello"));
          const outputRow = rows.findIndex((row) => row.trim() === "hello");
          const nextPromptRow = rows.findIndex(
            (row, index) => index > outputRow && row.includes("shush@") && row.includes(":~$"),
          );
          const statusRow = rows.find((row) => row.includes("0:bash*")) ?? "";

          return (
            commandRow >= 0 &&
            outputRow > commandRow &&
            nextPromptRow > outputRow &&
            !statusRow.includes("echo hello")
          );
        }, { timeout: 35_000 })
        .toBe(true);

      const after = await terminalText(page);
      await page.screenshot({ path: test.info().outputPath("after-approve-flow.png") });

      expect(after).not.toBe(before);
      expect(after).not.toContain("\\033\\");
      expect(after).not.toContain("_BEGIN_");
      expect(after).not.toContain("_END_");

      if (isStreamAssertEnabled()) {
        await expect.poll(() => streamContains(page, "echo hello")).toBe(true);
        await expect.poll(() => streamContains(page, "hello")).toBe(true);
        expect(await wsEventsContain(page, "\\033\\")).toBe(false);
        expect(await wsEventsContain(page, "_BEGIN_")).toBe(false);
        expect(await wsEventsContain(page, "_END_")).toBe(false);
      }
    } finally {
      await deleteSession(runtime.baseUrl(), session.id);
    }
  });

  test("remote monitor keeps LF-only command output aligned", async ({ page }) => {
    const sshConfig = remoteSshConfigPath();
    await access(sshConfig);
    const backend: Backend = {
      kind: "remote",
      host: process.env.SHUSH_E2E_REMOTE_HOST ?? "shush-docker",
      sshConfig,
      label: "remote-lf-alignment",
    };
    const session = await createSession(runtime.baseUrl(), backend);

    try {
      await installWsProbe(page);
      await page.goto(`${runtime.baseUrl()}/monitor/${encodeURIComponent(session.id)}`);
      await expect(page.locator(".monitor-page")).toBeVisible();

      await sendShellCommand(backend, session.name, "printf 'AA\\nBB\\nCC\\n'");

      await expect
        .poll(async () => {
          return await page.evaluate(() => {
            const rowEls = Array.from(
              document.querySelectorAll(".xterm-rows > div"),
            ) as HTMLDivElement[];
            const rows = rowEls.map((el) => el.textContent ?? "");
            return rows.join("\n");
          });
        })
        .toContain("\nBB");

      await expect
        .poll(async () => {
          return await page.evaluate(() => {
            const rowEls = Array.from(
              document.querySelectorAll(".xterm-rows > div"),
            ) as HTMLDivElement[];
            const rows = rowEls.map((el) => el.textContent ?? "");
            return rows.join("\n");
          });
        })
        .toContain("\nCC");
    } finally {
      await deleteSession(runtime.baseUrl(), session.id);
    }
  });
});

async function waitForServerReady(baseUrl: string, timeoutMs = 30_000): Promise<void> {
  const start = Date.now();
  while (Date.now() - start < timeoutMs) {
    try {
      const response = await fetch(`${baseUrl}/api/sessions`);
      if (response.ok) return;
    } catch {
      // retry
    }
    await sleep(250);
  }
  throw new Error("server did not become ready in time");
}

async function createSession(baseUrl: string, backend: Backend): Promise<Session> {
  const suffix = randomUUID().slice(0, 8);
  const response = await fetch(`${baseUrl}/api/sessions`, {
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

async function deleteSession(baseUrl: string, sessionId: string): Promise<void> {
  await fetch(`${baseUrl}/api/sessions/${sessionId}`, { method: "DELETE" });
}

async function submitCommand(baseUrl: string, sessionId: string, command: string): Promise<void> {
  const response = await fetch(`${baseUrl}/api/sessions/${sessionId}?action=submit`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ command }),
  });
  if (!response.ok) {
    throw new Error(`submit failed: ${response.status}`);
  }
}

async function approveCommand(baseUrl: string, sessionId: string): Promise<void> {
  const response = await fetch(`${baseUrl}/api/sessions/${sessionId}?action=approve`, {
    method: "POST",
  });
  if (!response.ok) {
    throw new Error(`approve failed: ${response.status}`);
  }
}

async function sessionExists(baseUrl: string, sessionId: string): Promise<boolean> {
  const response = await fetch(`${baseUrl}/api/sessions/${sessionId}`);
  return response.status === 200;
}

async function allocateFreePort(): Promise<number> {
  return await new Promise<number>((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close(() => reject(new Error("failed to allocate local port")));
        return;
      }
      const { port } = address;
      server.close((err) => {
        if (err) {
          reject(err);
        } else {
          resolve(port);
        }
      });
    });
    server.on("error", reject);
  });
}

async function sendVisibleLine(backend: Backend, sessionName: string, line: string): Promise<void> {
  const cmd = tmuxSendKeysCmd(sessionName, `echo ${line}`);
  if (backend.kind === "local") {
    await runCommand("bash", ["-lc", cmd], repoRoot);
  } else {
    await runCommand("ssh", ["-F", backend.sshConfig, backend.host, cmd], repoRoot);
  }

  await waitForCapturedLine(backend, sessionName, line);
}

async function sendShellCommand(
  backend: Backend,
  sessionName: string,
  command: string,
): Promise<void> {
  const cmd = tmuxSendKeysCmd(sessionName, command);
  if (backend.kind === "local") {
    await runCommand("bash", ["-lc", cmd], repoRoot);
  } else {
    await runCommand("ssh", ["-F", backend.sshConfig, backend.host, cmd], repoRoot);
  }
}

function tmuxSendKeysCmd(sessionName: string, shellCommand: string): string {
  const payload = shellQuote(shellCommand);
  return `tmux -L shush send-keys -t ${sessionName} ${payload} Enter`;
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

async function wsEventsContain(
  page: import("@playwright/test").Page,
  needle: string,
): Promise<boolean> {
  return await page.evaluate((target) => {
    const events =
      (window as unknown as { __shushWsEvents?: string[] }).__shushWsEvents ?? [];
    return events.some((raw) => raw.includes(target));
  }, needle);
}

async function terminalText(page: import("@playwright/test").Page): Promise<string> {
  const rows = await terminalRows(page);
  return rows.join("\n");
}

async function terminalRows(page: import("@playwright/test").Page): Promise<string[]> {
  return await page.evaluate(() => {
    const rowEls = Array.from(document.querySelectorAll(".xterm-rows > div")) as HTMLDivElement[];
    return rowEls.map((el) => el.textContent ?? "");
  });
}

function isRemoteEnabled(): boolean {
  return (process.env.SHUSH_E2E_REMOTE ?? "1") === "1";
}

function isStreamAssertEnabled(): boolean {
  return (process.env.SHUSH_E2E_ASSERT_STREAM ?? "1") === "1";
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
