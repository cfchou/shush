import { describe, expect, it } from "vitest";

import { reconnectDelayMs } from "./stream_utils";

describe("reconnectDelayMs", () => {
  it("starts at 1 second", () => {
    expect(reconnectDelayMs(0)).toBe(1000);
  });

  it("doubles on each attempt", () => {
    expect(reconnectDelayMs(1)).toBe(2000);
    expect(reconnectDelayMs(2)).toBe(4000);
    expect(reconnectDelayMs(3)).toBe(8000);
  });

  it("caps at 30 seconds", () => {
    expect(reconnectDelayMs(5)).toBe(30000);
    expect(reconnectDelayMs(20)).toBe(30000);
  });
});
