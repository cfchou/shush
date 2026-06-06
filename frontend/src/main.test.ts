// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const renderMonitorMock = vi.fn((root: HTMLElement, _sessionId?: string) => {
  root.innerHTML = `<div class="monitor-page-placeholder" data-session="${
    _sessionId ?? "none"
  }"></div>`;
});
const renderDashboardMock = vi.fn((root: HTMLElement) => {
  root.innerHTML = `<div class="dashboard-page-placeholder"></div>`;
});

vi.mock("./monitor/monitor", () => ({
  renderMonitor: renderMonitorMock,
}));

vi.mock("./dashboard/dashboard", () => ({
  renderDashboard: renderDashboardMock,
}));

describe("main route dispatch", () => {
  let locationGetterSpy: ReturnType<typeof vi.spyOn> | undefined;

  beforeEach(() => {
    renderMonitorMock.mockClear();
    renderDashboardMock.mockClear();
    document.body.innerHTML = '<div id="app"></div>';
  });

  afterEach(() => {
    locationGetterSpy?.mockRestore();
    document.body.innerHTML = "";
    vi.resetModules();
    vi.clearAllMocks();
  });

  async function bootstrapForPath(pathname: string): Promise<void> {
    locationGetterSpy?.mockRestore();
    locationGetterSpy = vi
      .spyOn(window, "location", "get")
      .mockReturnValue({ pathname } as Location);
    vi.resetModules();
    await import("./main");
  }

  it("renders dashboard on root", async () => {
    await bootstrapForPath("/");

    expect(renderDashboardMock).toHaveBeenCalledTimes(1);
    expect(renderMonitorMock).not.toHaveBeenCalled();
    expect(
      document.querySelector(".dashboard-page-placeholder"),
    ).not.toBeNull();
  });

  it("renders dashboard for non-monitor paths", async () => {
    await bootstrapForPath("/unknown-route");

    expect(renderDashboardMock).toHaveBeenCalledTimes(1);
    expect(renderMonitorMock).not.toHaveBeenCalled();
    expect(
      document.querySelector(".dashboard-page-placeholder"),
    ).not.toBeNull();
  });

  it("renders monitor scaffold for explicit monitor session route", async () => {
    await bootstrapForPath("/monitor/abc-123");

    expect(renderMonitorMock).toHaveBeenCalledTimes(1);
    expect(renderMonitorMock).toHaveBeenCalledWith(
      expect.any(HTMLDivElement),
      "abc-123",
    );
    expect(renderDashboardMock).not.toHaveBeenCalled();
  });

  it("renders idle monitor scaffold for monitor root", async () => {
    await bootstrapForPath("/monitor");

    expect(renderMonitorMock).toHaveBeenCalledTimes(1);
    expect(renderMonitorMock).toHaveBeenCalledWith(
      expect.any(HTMLDivElement),
      undefined,
    );
    expect(renderDashboardMock).not.toHaveBeenCalled();
    expect(
      document
        .querySelector(".monitor-page-placeholder")
        ?.getAttribute("data-session"),
    ).toBe("none");
  });
});
