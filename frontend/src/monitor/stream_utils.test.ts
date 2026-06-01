import { describe, expect, it } from "vitest";

import { normalizeLineEndings } from "./stream_utils";

describe("normalizeLineEndings", () => {
  it("converts lone LF to CRLF", () => {
    const input = new Uint8Array([0x61, 0x0a, 0x62, 0x0a]);
    const output = normalizeLineEndings(input);
    expect(Array.from(output)).toEqual([0x61, 0x0d, 0x0a, 0x62, 0x0d, 0x0a]);
  });

  it("preserves existing CRLF", () => {
    const input = new Uint8Array([0x61, 0x0d, 0x0a, 0x62]);
    const output = normalizeLineEndings(input);
    expect(Array.from(output)).toEqual([0x61, 0x0d, 0x0a, 0x62]);
  });
});
