/**
 * Company document header (printed reports): a stored value that is missing, malformed or
 * partially typed never breaks printing; it falls back field by field to an empty header.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getSetting,
  parseDocumentHeaderValue,
  readDocumentBrandingLogoPreview,
} from "@/services/settings-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

const EMPTY = {
  version: 1,
  content_json: { type: "doc", content: [{ type: "paragraph" }] },
  html: "",
  logo_relative_path: null,
};

describe("parseDocumentHeaderValue", () => {
  it.each([
    ["no value", null],
    ["undefined", undefined],
    ["blank text", "   "],
    ["invalid JSON", "{not json"],
    ["JSON null", "null"],
  ])("returns an empty header for %s", (_label, raw) => {
    expect(parseDocumentHeaderValue(raw)).toEqual(EMPTY);
  });

  it("keeps every well-typed field", () => {
    const value = {
      version: 3,
      content_json: { type: "doc", content: [] },
      html: "<p>Acme</p>",
      logo_relative_path: "branding/logo.png",
    };
    expect(parseDocumentHeaderValue(JSON.stringify(value))).toEqual(value);
  });

  it("falls back field by field when a field has the wrong type", () => {
    expect(
      parseDocumentHeaderValue(
        JSON.stringify({ version: "2", html: 42, logo_relative_path: false }),
      ),
    ).toEqual(EMPTY);
  });

  it("returns a fresh object each time (callers may mutate it)", () => {
    const a = parseDocumentHeaderValue(null);
    a.html = "changed";
    expect(parseDocumentHeaderValue(null).html).toBe("");
  });
});

describe("settings reads that may find nothing", () => {
  beforeEach(() => ipc.invoke.mockReset());

  it("returns null when the setting or the logo preview does not exist", async () => {
    ipc.invoke.mockResolvedValue(null);
    expect(await getSetting("document.header", "tenant")).toBeNull();
    expect(ipc.invoke).toHaveBeenCalledWith("get_setting", {
      key: "document.header",
      scope: "tenant",
    });
    expect(await readDocumentBrandingLogoPreview()).toBeNull();
  });
});
