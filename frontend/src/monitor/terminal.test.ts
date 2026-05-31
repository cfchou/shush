import { describe, expect, it } from "vitest";

import { decodeBase64Bytes } from "./stream_utils";

describe("decodeBase64Bytes", () => {
  it("decodes plain ascii payload", () => {
    const bytes = decodeBase64Bytes("aGVsbG8=");
    expect(Array.from(bytes)).toEqual([104, 101, 108, 108, 111]);
  });

  it("decodes ansi escape sequences", () => {
    const bytes = decodeBase64Bytes("G1sySg==");
    expect(Array.from(bytes)).toEqual([27, 91, 50, 74]);
  });
});
