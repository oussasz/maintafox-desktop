// src/i18n/__tests__/no-duplicate-keys.test.ts
// A key written twice in the same object of a catalogue is silently resolved by JSON.parse (the
// last one wins), so the newer wording can be lost without any other test noticing. Scan the
// raw text of every catalogue and fail on any duplicate key.

import fs from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

/** Duplicate keys of a JSON text, as `path.key`. Assumes the text is valid JSON. */
export function duplicateKeys(text: string): string[] {
  const dupes: string[] = [];
  // One frame per open object or array: keys seen (objects only) and the current key path.
  const stack: { keys: Set<string> | null; path: string }[] = [];
  let pendingKey: string | null = null;
  let i = 0;
  const readString = (): string => {
    let out = "";
    i += 1; // opening quote
    while (i < text.length) {
      const ch = text[i] ?? "";
      if (ch === "\\") {
        out += text.slice(i, i + 2);
        i += 2;
        continue;
      }
      if (ch === '"') {
        i += 1;
        return out;
      }
      out += ch;
      i += 1;
    }
    return out;
  };
  while (i < text.length) {
    const ch = text[i] ?? "";
    if (ch === '"') {
      const value = readString();
      // A string is a key when the next non-space character is a colon.
      let j = i;
      while (j < text.length && /\s/.test(text[j] ?? "")) j += 1;
      const top = stack[stack.length - 1];
      if (text[j] === ":" && top?.keys) {
        const at = top.path ? `${top.path}.${value}` : value;
        if (top.keys.has(value)) dupes.push(at);
        top.keys.add(value);
        pendingKey = value;
      }
      continue;
    }
    if (ch === "{" || ch === "[") {
      const parent = stack[stack.length - 1];
      const segment = pendingKey ?? "";
      const base = parent?.path ? `${parent.path}${segment ? `.${segment}` : ""}` : segment;
      stack.push({ keys: ch === "{" ? new Set() : null, path: base });
      pendingKey = null;
    } else if (ch === "}" || ch === "]") {
      stack.pop();
      pendingKey = null;
    } else if (ch === ",") {
      pendingKey = null;
    }
    i += 1;
  }
  return dupes;
}

function catalogues(): string[] {
  const root = path.resolve(process.cwd(), "src/i18n");
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      if (entry.isDirectory() && entry.name !== "__tests__") walk(full);
      else if (entry.isFile() && entry.name.endsWith(".json")) out.push(full);
    }
  };
  walk(root);
  return out;
}

describe("i18n catalogues", () => {
  it("finds a duplicate key (self-check)", () => {
    expect(duplicateKeys('{"a": {"x": "1", "y": "2", "x": "3"}, "b": "4"}')).toEqual(["a.x"]);
    expect(duplicateKeys('{"a": {"x": "1"}, "b": {"x": "2"}}')).toEqual([]);
  });

  it("have no key written twice in the same object", () => {
    const files = catalogues();
    expect(files.length).toBeGreaterThan(10);
    const problems = files.flatMap((f) =>
      duplicateKeys(fs.readFileSync(f, "utf8")).map(
        (k) => `${path.relative(process.cwd(), f)}: ${k}`,
      ),
    );
    expect(problems).toEqual([]);
  });
});
