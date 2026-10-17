import { describe, expect, it } from "vitest";

import { assetQrPayload, parseAssetScan } from "@/lib/asset-qr";

describe("asset QR labels", () => {
  it("carry the asset code, read back exactly", () => {
    const payload = assetQrPayload("PMP-101/A é");
    expect(payload).toBe("maintafox://asset/code/PMP-101%2FA%20%C3%A9");
    expect(parseAssetScan(payload)).toEqual({ kind: "code", code: "PMP-101/A é" });
    expect(parseAssetScan(`  ${payload.toUpperCase().replace("%2FA%20%C3%A9", "%2Fa")}  `)).toEqual(
      { kind: "code", code: "PMP-101/a" },
    );
  });

  it("report labels of the former id format instead of guessing an asset", () => {
    expect(parseAssetScan("maintafox://asset/42")).toEqual({ kind: "legacy_label" });
  });

  it("search anything else as typed text", () => {
    expect(parseAssetScan(" PMP-101 ")).toEqual({ kind: "text", text: "PMP-101" });
    expect(parseAssetScan("maintafox://asset/code/%E0%A4%A")).toEqual({
      kind: "text",
      text: "maintafox://asset/code/%E0%A4%A",
    });
    expect(parseAssetScan("maintafox://asset/code/")).toEqual({
      kind: "text",
      text: "maintafox://asset/code/",
    });
  });
});
