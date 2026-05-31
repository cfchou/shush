// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

  it("renders escaped session id and uses ws URL on http", () => {
    setLocation("http:", "localhost:5173");
    const root = createRoot();

    renderMonitor(root, "id<unsafe>&ok");

    const sessionText = root.querySelector(".session-id")?.innerHTML;
    expect(sessionText).toContain("id&lt;unsafe&gt;&amp;ok");
    expect(FakeWebSocket.instances[0]?.url).toBe(
      "ws://localhost:5173/api/sessions/id%3Cunsafe%3E%26ok/stream",
    );
  });

  it("uses wss URL on https", () => {
    setLocation("https:", "monitor.example");
    const root = createRoot();

    renderMonitor(root, "session");

    expect(FakeWebSocket.instances[0]?.url).toBe(
      "wss://monitor.example/api/sessions/session/stream",
    );
  });

  it("ignores invalid message payloads", () => {
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root, "session");
    const socket = FakeWebSocket.instances[0];

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

  it("applies snapshot and terminal messages with expected terminal effects", () => {
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root, "session");
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

  it("schedules reconnect with backoff and resets attempts after open", () => {
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root, "session");
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

  it("closes socket on error", () => {
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root, "session");
    const socket = FakeWebSocket.instances[0];

    socket.dispatch("error");

    expect(socket.close).toHaveBeenCalledTimes(1);
  });

  it("does not reconnect after beforeunload", () => {
    setLocation("http:");
    const root = createRoot();
    renderMonitor(root, "session");
    const socket = FakeWebSocket.instances[0];

    beforeUnloadHandler?.();
    socket.dispatch("close");

    expect(timeoutSpy).not.toHaveBeenCalled();
  });
});
