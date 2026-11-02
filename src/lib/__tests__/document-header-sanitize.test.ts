// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import { HEADER_IMAGE_MAX_BYTES, dataUrlBytes, scaledSize } from "@/lib/document-header-image";
import {
  DOCUMENT_HEADER_ALLOWED_ATTRIBUTES,
  DOCUMENT_HEADER_ALLOWED_TAGS,
  DOCUMENT_HEADER_MAX_BYTES,
  documentHeaderIssue,
  sanitizeDocumentHeaderHtml,
} from "@/lib/document-header-sanitize";

const CATALOG = readFileSync(resolve(process.cwd(), "src-tauri/src/settings/catalog.rs"), "utf8");

function rustList(name: string): string[] {
  const body = new RegExp(`const ${name}: &\\[&str\\] = &\\[([^\\]]*)\\];`).exec(CATALOG)?.[1];
  if (!body) throw new Error(`${name} not found in catalog.rs`);
  return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1] as string);
}

/** Same sample as the engine's unit test `header_sample_from_the_editor_is_accepted`. */
const SAMPLE = [
  '<h2 style="text-align: center">Atelier Nord</h2>',
  "<p><strong>Maintafox</strong> <em>GMAO</em> <u>usine</u> <s>x</s></p>",
  "<ul><li>Ligne 1</li></ul><ol><li>Ligne 2</li></ol>",
  '<table style="width: 100%"><thead><tr><th colspan="2">Site</th></tr></thead>',
  '<tbody><tr><td rowspan="1" align="left">A &amp; B &lt; C</td><td>1</td></tr></tbody></table>',
  '<p><br></p><img src="data:image/png;base64,iVBORw0KGgo=" alt="Logo" width="80" height="40">',
  '<script>alert(1)</script><!-- note --><p onclick="x">ok</p>',
].join("");

describe("document header allow-list", () => {
  it("matches the engine's lists (catalog.rs)", () => {
    expect([...DOCUMENT_HEADER_ALLOWED_TAGS].sort()).toEqual(rustList("HEADER_TAGS").sort());
    expect([...DOCUMENT_HEADER_ALLOWED_ATTRIBUTES].sort()).toEqual(
      rustList("HEADER_ATTRIBUTES").sort(),
    );
  });

  it("uses the engine's size limit", () => {
    const rust = /pub const HEADER_MAX_BYTES: usize = ([0-9_]+);/.exec(CATALOG)?.[1];
    expect(Number((rust ?? "").replace(/_/g, ""))).toBe(DOCUMENT_HEADER_MAX_BYTES);
  });

  it("produces output the engine's rules accept (text, table, image, formatting)", () => {
    const out = sanitizeDocumentHeaderHtml(SAMPLE);
    const tags = rustList("HEADER_TAGS");
    const attrs = rustList("HEADER_ATTRIBUTES");
    const doc = new DOMParser().parseFromString(`<body>${out}</body>`, "text/html");
    const all = Array.from(doc.body.querySelectorAll("*"));
    expect(all.length).toBeGreaterThan(10);
    for (const el of all) {
      expect(tags).toContain(el.tagName.toLowerCase());
      for (const a of Array.from(el.attributes)) expect(attrs).toContain(a.name);
    }
    expect(out).not.toContain("<!--");
    expect(out).not.toContain("<script");
    expect(out).not.toContain("onclick");
    expect(out).toContain("<table");
    expect(out).toContain("<img");
    expect(documentHeaderIssue(out)).toBeNull();
  });
});

describe("documentHeaderIssue", () => {
  it("names the size and the limit of a header over the limit", () => {
    const big = `<p>${"a".repeat(340_000)}</p>`;
    expect(documentHeaderIssue(big)).toEqual({
      code: "GATE_SET_HEADER_TOO_LARGE",
      params: { size_kb: "341", max_kb: "200" },
    });
  });
});

describe("document header image helpers", () => {
  it("measures a data URL in bytes", () => {
    expect(dataUrlBytes("data:image/png;base64,QUJD")).toBe(3);
    expect(dataUrlBytes("data:image/png;base64,QUI=")).toBe(2);
  });

  it("limits one picture to a share of the header limit", () => {
    expect(HEADER_IMAGE_MAX_BYTES).toBeLessThan(DOCUMENT_HEADER_MAX_BYTES);
  });

  it("never draws wider than the print width", () => {
    expect(scaledSize(1200, 600, 1)).toEqual([600, 300]);
    expect(scaledSize(200, 100, 0.5)).toEqual([100, 50]);
  });
});
