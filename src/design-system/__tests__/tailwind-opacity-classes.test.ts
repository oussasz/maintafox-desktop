/**
 * Every colour class written with an opacity modifier (`bg-muted/50`, `bg-tone-danger-bg/40`, …)
 * must produce CSS. On Tailwind 3 a colour defined as a CSS variable silently produced nothing with
 * a modifier; `tailwind.config.ts` now mixes those colours with transparent. This test compiles every
 * such class found in `src` and fails on any that would be dropped (unknown colour, off-scale
 * opacity, variable colour without the alpha function).
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import postcss from "postcss";
import tailwind, { type Config } from "tailwindcss";
import { describe, expect, it } from "vitest";

import config from "../../../tailwind.config";

const ROOT = join(__dirname, "..", "..");
const CLASS_RE =
  /(?<![\w-])(?:bg|text|border|ring|fill|stroke|divide|outline|decoration|placeholder|caret|accent|shadow|border-[trblxy])-[a-z][a-z0-9-]*\/[0-9]+(?![\w-])/g;

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const name of readdirSync(dir)) {
    if (name === "__tests__" || name === "node_modules") continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...sourceFiles(path));
    else if (/\.(tsx?|css)$/.test(name)) out.push(path);
  }
  return out;
}

function escapeClass(cls: string): string {
  return `.${cls.replace(/[/.:[\]]/g, (c) => `\\${c}`)}`;
}

describe("Tailwind colour classes with an opacity modifier", () => {
  it("all generate CSS", async () => {
    const classes = new Set<string>();
    for (const file of sourceFiles(ROOT)) {
      for (const m of readFileSync(file, "utf8").matchAll(CLASS_RE)) classes.add(m[0]);
    }
    expect(classes.size).toBeGreaterThan(50);

    const html = `<div class="${[...classes].join(" ")}"></div>`;
    const result = await postcss([
      tailwind({ ...config, content: [{ raw: html, extension: "html" }] } as Config),
    ]).process("@tailwind utilities;", { from: undefined });

    const dropped = [...classes].filter((cls) => !result.css.includes(`${escapeClass(cls)} `));
    expect(dropped).toEqual([]);
  }, 60_000);
});
