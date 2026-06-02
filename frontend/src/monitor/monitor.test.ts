// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session } from "../types";

const terminalCalls: string[] = [];
const terminalSpies = {
  clear: vi.fn(() => {
    terminalCalls.push("clear");
  }),
  write: vi.fn((_: Uint8Array) => {
    terminalCalls.push("write");
  }),
  fit: vi.fn(() => {
    terminalCalls.push("fit");
  }),
};

const listSessionsMock = vi.fn(async () => [] as Session[]);
const getSessionMock = vi.fn(async (_id: string): Promise<Session> => {
  throw new Error("not found");
});

vi.mock("../api", () => ({
  listSessions: (...args: unknown[]) => listSessionsMock(...args),
  getSession: (...args: unknown[]) => getSessionMock(...args),
}));

vi.mock("./terminal", () => ({
  TerminalView: class {
    clear(): void {
      terminalSpies.clear();
    }

    write(data: Uint8Array): void {
      terminalSpies.write(data);
    }

    fit(): void {
      terminalSpies.fit();
    }
  },
}));

import { renderMonitor } from "./monitor";

type Listener = (event?: unknown) => void;

class FakeWebSocket {
  static instances: FakeWebSocket[] = [];

  readonly url: string;
  close = vi.fn();
  private listeners = new Map<string, Listener[]>();

  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }

  addEventListener(type: string, listener: Listener): void {
    const existing = this.listeners.get(type) ?? [];
    existing.push(listener);
    this.listeners.set(type, existing);
  }

  dispatch(type: string, event?: unknown): void {
    const listeners = this.listeners.get(type) ?? [];
    for (const listener of listeners) {
      listener(event);
    }
  }
}

function flushPromises(): Promise<void> {
  return Promise.resolve();
}

describe("renderMonitor", () => {
  const originalWebSocket = globalThis.WebSocket;
  let timeoutSpy: ReturnType<typeof vi.spyOn>;
  let addEventListenerSpy: ReturnType<typeof vi.spyOn>;
  let locationGetterSpy: ReturnType<typeof vi.spyOn>;
  let beforeUnloadHandler: (() => void) | undefined;
  const realAddEventListener = window.addEventListener.bind(window);

  beforeEach(() => {
    FakeWebSocket.instances = [];
    terminalCalls.length = 0;
    terminalSpies.clear.mockClear();
    terminalSpies.write.mockClear();
    terminalSpies.fit.mockClear();
    listSessionsMock.mockReset();
    getSessionMock.mockReset();
    listSessionsMock.mockResolvedValue([]);
    getSessionMock.mockRejectedValue(new Error("not found"));

    globalThis.WebSocket = FakeWebSocket as unknown as typeof WebSocket;
    timeoutSpy = vi
      .spyOn(window, "setTimeout")
      .mockImplementation(((_: TimerHandler) => 0) as typeof window.setTimeout);
    addEventListenerSpy = vi
      .spyOn(window, "addEventListener")
      .mockImplementation(
        (
          type: string,
          listener: EventListenerOrEventListenerObject,
          options?: boolean | AddEventListenerOptions,
        ) => {
          if (type === "beforeunload" && typeof listener === "function") {
            beforeUnloadHandler = listener as () => void;
          }
          return realAddEventListener(type, listener, options);
        },
      );
  });

  afterEach(() => {
    timeoutSpy.mockRestore();
    addEventListenerSpy.mockRestore();
    locationGetterSpy?.mockRestore();
    beforeUnloadHandler = undefined;
    globalThis.WebSocket = originalWebSocket;
    document.body.innerHTML = "";
  });

  function setLocation(
    protocol: "http:" | "https:",
    host = "example.test:8100",
  ): void {
    locationGetterSpy?.mockRestore();
    locationGetterSpy = vi
      .spyOn(window, "location", "get")
      .mockReturnValue({ protocol, host } as Location);
  }

  function createRoot(): HTMLElement {
    const root = document.createElement("div");
    document.body.appendChild(root);
    return root;
  }

  function setViewportWidth(value: number): void {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value,
    });
  }

  function createActiveSession(id: string): Session {
    return {
      id,
      name: `Session ${id}`,
      host: "local",
      state: "idle",
      yolo: false,
      current_command: null,
      created_at: new Date().toISOString(),
    };
  }

  it("renders idle scaffold with placeholder and no websocket connection", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    const root = createRoot();

    renderMonitor(root);
    await flushPromises();

    expect(root.querySelector(".monitor-shell")).not.toBeNull();
    expect(root.querySelector(".monitor-title")?.textContent).toBe(
      "No session selected",
    );
    expect(root.querySelector(".monitor-terminal-placeholder")).not.toBeNull();
    expect(
      root
        .querySelector(".monitor-terminal-placeholder")
        ?.classList.contains("is-hidden"),
    ).toBe(false);
    expect(FakeWebSocket.instances).toHaveLength(0);
    expect(
      root
        .querySelector(".monitor-command-empty")
        ?.classList.contains("is-visible"),
    ).toBe(true);
  });

  it("renders active monitor with session metadata and loads sessions list", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    const session = createActiveSession("abc");
    listSessionsMock.mockResolvedValue([session, createActiveSession("def")]);
    getSessionMock.mockResolvedValue(session);

    const root = createRoot();

    renderMonitor(root, "abc");
    await vi.waitFor(() => {
      expect(getSessionMock).toHaveBeenCalledWith("abc");
    });

    expect(listSessionsMock).toHaveBeenCalledTimes(1);
    expect(getSessionMock).toHaveBeenCalledWith("abc");
    await vi.waitFor(() => {
      expect(FakeWebSocket.instances).toHaveLength(1);
    });

    expect(FakeWebSocket.instances).toHaveLength(1);
    expect(root.querySelector(".session-id")?.textContent).toContain(
      "Session: abc",
    );
    expect(root.querySelector(".monitor-title")?.textContent).toBe(
      "Session abc",
    );
    const activeRow = root.querySelector(".monitor-session-item.is-active");
    expect(activeRow).not.toBeNull();
    expect(
      activeRow?.querySelector(".monitor-session-item-title")?.textContent,
    ).toContain("Session abc");
  });

  it("keeps sidebars collapsible and writes collapsed state to shell tokens", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root);
    await flushPromises();

    const shell = root.querySelector<HTMLElement>("#monitor-shell");
    expect(shell).not.toBeNull();
    expect(shell?.dataset.leftHidden).toBe("false");
    expect(shell?.dataset.rightHidden).toBe("false");

    const leftToggle = root.querySelector<HTMLElement>("#toggle-left");
    const rightToggle = root.querySelector<HTMLElement>("#toggle-right");

    leftToggle?.click();
    expect(shell?.dataset.leftHidden).toBe("true");
    expect(
      root
        .querySelector<HTMLElement>("#monitor-left-badge-banner")
        ?.getAttribute("data-visible"),
    ).toBe("true");

    rightToggle?.click();
    expect(shell?.dataset.rightHidden).toBe("true");
    expect(
      root
        .querySelector<HTMLElement>("#monitor-right-badge-banner")
        ?.getAttribute("data-visible"),
    ).toBe("true");
  });

  it("toggles YOLO switch state locally", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root);
    await flushPromises();

    const control = root.querySelector<HTMLElement>("#yolo-control");
    const label = root.querySelector<HTMLElement>("#yolo-label");
    const sw = root.querySelector<HTMLElement>("#yolo-switch");

    expect(control).not.toBeNull();
    expect(sw?.classList.contains("is-on")).toBe(false);
    expect(label?.textContent).toBe("YOLO Mode Off");

    control?.dispatchEvent(new MouseEvent("click"));
    expect(sw?.classList.contains("is-on")).toBe(true);
    expect(label?.textContent).toBe("YOLO Mode Active");

    control?.dispatchEvent(new MouseEvent("click"));
    expect(sw?.classList.contains("is-on")).toBe(false);
    expect(label?.textContent).toBe("YOLO Mode Off");
  });

  it("ignores invalid terminal messages", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    getSessionMock.mockResolvedValue(createActiveSession("session"));
    const root = createRoot();
    renderMonitor(root, "session");
    await vi.waitFor(() => {
      expect(FakeWebSocket.instances).toHaveLength(1);
    });
    const socket = FakeWebSocket.instances[0];
    expect(socket).toBeDefined();

    socket.dispatch("message", { data: new Uint8Array([1, 2, 3]) });
    socket.dispatch("message", { data: "not-json" });
    socket.dispatch("message", { data: JSON.stringify({ type: "terminal" }) });
    socket.dispatch("message", {
      data: JSON.stringify({ type: "terminal", data: 1 }),
    });

    expect(terminalSpies.clear).not.toHaveBeenCalled();
    expect(terminalSpies.write).not.toHaveBeenCalled();
    expect(terminalSpies.fit).not.toHaveBeenCalled();
  });

  it("writes terminal output for valid snapshot and terminal frames", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    getSessionMock.mockResolvedValue(createActiveSession("session"));
    const root = createRoot();

    renderMonitor(root, "session");
    await vi.waitFor(() => {
      expect(FakeWebSocket.instances).toHaveLength(1);
    });
    const socket = FakeWebSocket.instances[0];

    socket.dispatch("message", {
      data: JSON.stringify({ type: "snapshot", data: "aGVsbG8=" }),
    });
    socket.dispatch("message", {
      data: JSON.stringify({ type: "terminal", data: "d29ybGQ=" }),
    });

    expect(terminalCalls).toEqual(["clear", "write", "fit", "write"]);
    expect(Array.from(terminalSpies.write.mock.calls[0][0])).toEqual([
      104, 101, 108, 108, 111,
    ]);
    expect(Array.from(terminalSpies.write.mock.calls[1][0])).toEqual([
      119, 111, 114, 108, 100,
    ]);
  });

  it("schedules reconnect attempts and resets after open", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    getSessionMock.mockResolvedValue(createActiveSession("session"));
    const root = createRoot();
    renderMonitor(root, "session");
    await vi.waitFor(() => {
      expect(FakeWebSocket.instances).toHaveLength(1);
    });

    const first = FakeWebSocket.instances[0];
    first.dispatch("close");
    expect(timeoutSpy).toHaveBeenNthCalledWith(1, expect.any(Function), 1000);

    const reconnect1 = timeoutSpy.mock.calls[0][0] as () => void;
    reconnect1();
    const second = FakeWebSocket.instances[1];
    second.dispatch("close");
    expect(timeoutSpy).toHaveBeenNthCalledWith(2, expect.any(Function), 2000);

    second.dispatch("open");
    const reconnect2 = timeoutSpy.mock.calls[1][0] as () => void;
    reconnect2();
    const third = FakeWebSocket.instances[2];
    third.dispatch("close");
    expect(timeoutSpy).toHaveBeenNthCalledWith(3, expect.any(Function), 1000);
  });

  it("does not reconnect after beforeunload", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    getSessionMock.mockResolvedValue(createActiveSession("session"));
    const root = createRoot();
    renderMonitor(root, "session");
    await vi.waitFor(() => {
      expect(FakeWebSocket.instances).toHaveLength(1);
    });

    const socket = FakeWebSocket.instances[0];
    beforeUnloadHandler?.();
    socket.dispatch("close");

    expect(timeoutSpy).not.toHaveBeenCalled();
  });

  it("errors gracefully when stream message is received before sockets are available", async () => {
    setViewportWidth(1920);
    setLocation("http:");
    getSessionMock.mockResolvedValue(createActiveSession("session"));
    const root = createRoot();

    renderMonitor(root, "session");
    await vi.waitFor(() => {
      expect(FakeWebSocket.instances).toHaveLength(1);
    });
    const socket = FakeWebSocket.instances[0];

    socket.dispatch("open");
    socket.dispatch("message", {
      data: JSON.stringify({ type: "terminal", data: "aGVsbG8=" }),
    });

    expect(terminalCalls).toContain("write");
  });
});
