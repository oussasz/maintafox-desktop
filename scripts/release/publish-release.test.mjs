import { describe, expect, it } from "vitest";

import { checkApiBase } from "./publish-release.mjs";

describe("checkApiBase", () => {
  it("accepts https, without the trailing slash", () => {
    expect(checkApiBase(" https://api.maintafox.systems/ ")).toEqual({
      base: "https://api.maintafox.systems",
    });
  });

  it("accepts plain http only for a local test server", () => {
    for (const local of ["http://localhost:3000", "http://127.0.0.1:8080", "http://[::1]:9000"]) {
      expect(checkApiBase(local)).toEqual({ base: local });
    }
  });

  it("refuses anything else: the release token would travel in clear", () => {
    for (const bad of [
      "http://api.maintafox.systems",
      "http://localhost.evil.example",
      "ftp://api.maintafox.systems",
      "not a url",
      "",
      undefined,
    ]) {
      expect(checkApiBase(bad).error, String(bad)).toBeTruthy();
    }
  });
});
