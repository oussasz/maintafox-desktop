// Every GATE_* refusal code the backend can send has a French and an English text under
// `errors:gateError`: an untranslated code would reach the user as a bare internal code.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import enErrors from "../en/errors.json";
import frErrors from "../fr/errors.json";
import { ISSUE_NAMESPACES } from "../namespaces";

function rustFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const full = join(dir, name);
    if (statSync(full).isDirectory()) return rustFiles(full);
    return name.endsWith(".rs") ? [full] : [];
  });
}

function gateCodesInBackend(): string[] {
  const codes = new Set<string>();
  for (const file of rustFiles(join(process.cwd(), "src-tauri", "src"))) {
    const source = readFileSync(file, "utf8");
    for (const match of source.matchAll(/"\{?(GATE_[A-Z0-9_]+)/g)) {
      if (match[1]) codes.add(match[1]);
    }
  }
  return [...codes].sort();
}

describe("backend GATE codes are translated", () => {
  const codes = gateCodesInBackend();

  it("finds the codes in the backend sources", () => {
    expect(codes.length).toBeGreaterThan(50);
  });

  it("has a French text for each", () => {
    const gate = frErrors.gateError as Record<string, string>;
    expect(codes.filter((c) => !gate[c])).toEqual([]);
  });

  it("has an English text for each", () => {
    const gate = enErrors.gateError as Record<string, string>;
    expect(codes.filter((c) => !gate[c])).toEqual([]);
  });
});

// AU-124: the codes of structured refusals (`org_validation_failed`, `AppValidationIssue`, and the
// small helpers that forward their `code` parameter to them) have a French and an English text in
// one of the places the screen looks (`src/lib/format-org-validation-issue.ts`): otherwise the user
// reads the backend sentence, in one language only.

/**
 * Codes still without a text in both languages. Each entry is listed with its owner; the list only
 * shrinks (a code that gets its texts must leave it).
 */
const ISSUE_CODES_WITHOUT_TEXT: readonly string[] = [];

const BASE_ISSUE_BUILDERS = [
  "org_validation_failed",
  "AppValidationIssue::error",
  "AppValidationIssue::warning",
];

function backendSources(): string[] {
  return rustFiles(join(process.cwd(), "src-tauri", "src"))
    .filter(
      (f) => !/(?:^|[\\/])(?:[^\\/]*tests?\.rs|[^\\/]*_tests[^\\/]*|test_support\.rs)$/.test(f),
    )
    .filter((f) => !/[\\/]migrations[\\/]/.test(f))
    .map((f) => readFileSync(f, "utf8"));
}

/** Functions that take a `code: &str` parameter and pass it on as the code of a structured refusal. */
function issueBuilders(sources: string[]): string[] {
  const names = new Set(BASE_ISSUE_BUILDERS);
  const forwards = /(?:org_validation_failed|AppValidationIssue::(?:error|warning))\(\s*code\b/g;
  for (const source of sources) {
    for (const m of source.matchAll(forwards)) {
      const before = source.slice(0, m.index);
      const decl = [...before.matchAll(/\bfn\s+(\w+)\s*(?:<[^>{]*>)?\s*\(/g)].pop();
      if (!decl?.[1] || decl.index === undefined) continue;
      if (/\bcode\s*:\s*&/.test(before.slice(decl.index))) names.add(decl[1]);
    }
  }
  return [...names];
}

function structuredIssueCodes(): string[] {
  const sources = backendSources();
  const constDecl = /const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"]+)"/g;
  // Every value each constant name has across the backend: a name used in a file resolves to that
  // file's own constant, otherwise to every value it has elsewhere (a name defined in several files
  // with different values is checked under each of them, never under none).
  const everywhere = new Map<string, Set<string>>();
  for (const source of sources) {
    for (const m of source.matchAll(constDecl)) {
      if (m[1] && m[2]) everywhere.set(m[1], (everywhere.get(m[1]) ?? new Set()).add(m[2]));
    }
  }
  const call = new RegExp(
    String.raw`(?<![\w.])(?:[a-z_]+::)*(?:${issueBuilders(sources).join("|")})\(\s*(?:"([A-Z][A-Z0-9_]+)"|(?:[a-z_]+::)*([A-Z][A-Z0-9_]*)\b)`,
    "g",
  );
  const codes = new Set<string>();
  for (const source of sources) {
    const local = new Map<string, string>();
    for (const m of source.matchAll(constDecl)) {
      if (m[1] && m[2]) local.set(m[1], m[2]);
    }
    for (const m of source.matchAll(call)) {
      const named = m[2];
      const values = m[1]
        ? [m[1]]
        : named
          ? local.has(named)
            ? [local.get(named) ?? ""]
            : [...(everywhere.get(named) ?? [])]
          : [];
      for (const code of values) {
        if (/^[A-Z][A-Z0-9_]+$/.test(code) && !code.startsWith("GATE_")) codes.add(code);
      }
    }
  }
  return [...codes].sort();
}

type Json = Record<string, unknown>;

function loadNamespace(locale: string, ns: string): Json {
  for (const dir of [join("src", "i18n", locale), join("src", "i18n", "locale-data", locale)]) {
    const file = join(process.cwd(), dir, `${ns}.json`);
    try {
      return JSON.parse(readFileSync(file, "utf8")) as Json;
    } catch {
      // not in this folder
    }
  }
  return {};
}

function textAt(doc: Json, key: string): unknown {
  return key.split(".").reduce<unknown>((node, part) => (node as Json | undefined)?.[part], doc);
}

function hasIssueText(locale: string, code: string): boolean {
  const org = loadNamespace(locale, "org");
  const errors = loadNamespace(locale, "errors");
  const keys: Array<[Json, string]> = [
    ...["governance.issues", "errors", "preview.issues"].map((p): [Json, string] => [
      org,
      `${p}.${code}`,
    ]),
    ...ISSUE_NAMESPACES.map((ns): [Json, string] => [loadNamespace(locale, ns), `errors.${code}`]),
    [errors, `scope.${code}`],
    [errors, `gateError.${code}`],
  ];
  return keys.some(([doc, key]) => {
    const text = textAt(doc, key);
    return typeof text === "string" && text.trim() !== "";
  });
}

describe("structured refusal codes are translated (AU-124)", () => {
  const codes = structuredIssueCodes();

  it("finds the codes in the backend sources", () => {
    expect(codes.length).toBeGreaterThan(250);
  });

  it("has a French and an English text for each, apart from the listed ones", () => {
    const missing = codes.filter((c) => !hasIssueText("fr", c) || !hasIssueText("en", c));
    expect(missing).toEqual([...ISSUE_CODES_WITHOUT_TEXT].sort());
  });
});
