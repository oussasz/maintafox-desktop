/**
 * Contract test: every `invoke("<command>", { …keys })` sends the argument keys the Rust
 * command expects.
 *
 * Tauri v2 maps a Rust parameter `notification_id` to the JS key `notificationId` (camelCase)
 * unless the command is declared `#[tauri::command(rename_all = "snake_case")]`. Sending
 * `{ notification_id }` makes the backend reject the call ("missing required key
 * notificationId"), which in the UI is a button that silently does nothing.
 *
 * Both sides are read as text: the Rust command signatures under src-tauri/src and every
 * invoke call (plus local wrappers such as `invokeParsed("cmd", args, schema)`) under src/.
 * The fields inside a struct parameter follow that struct's own serde attributes (snake_case
 * unless the struct says `rename_all = "camelCase"`); those are checked for casing slips
 * against the TypeScript type (or inline object) the frontend passes.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";

import { describe, expect, it } from "vitest";

const REPO = resolve(__dirname, "../../..");
const RUST_SRC = join(REPO, "src-tauri/src");
const TS_SRC = join(REPO, "src");

/** Parameters Tauri injects itself; they are never sent from JS. */
const INJECTED_TYPE = /\b(State|AppHandle|Window|WebviewWindow|Webview|Request)\b/;

interface RustParam {
  name: string;
  optional: boolean;
  ty: string;
}
interface RustCommand {
  file: string;
  params: RustParam[];
  renameAll: string | null;
}
interface InvokeCall {
  where: string;
  cmd: string;
  kind: "none" | "literal" | "expr";
  keys: string[];
  spreads: string[];
  /** Top-level key → value expression (`{ input }` → "input"). */
  values: Map<string, string>;
  /** Parameter name → declared type name, for the function that makes the call. */
  scope: Map<string, string>;
}

function listFiles(dir: string, exts: string[]): string[] {
  return (readdirSync(dir, { recursive: true }) as string[])
    .map((p) => join(dir, p))
    .filter((p) => exts.some((e) => p.endsWith(e)));
}

/** Index of the bracket closing the one at `open` (skips string literals in `quotes`). */
function closingIndex(src: string, open: number, o: string, c: string, quotes = "\"'`"): number {
  let depth = 0;
  let quote: string | null = null;
  for (let i = open; i < src.length; i++) {
    const ch = src[i] as string;
    if (quote) {
      if (ch === "\\") i++;
      else if (ch === quote) quote = null;
      continue;
    }
    if (o !== "<" && quotes.includes(ch)) quote = ch;
    else if (ch === o) depth++;
    else if (ch === c && --depth === 0) return i;
  }
  return -1;
}

/** Split on commas that are not nested inside brackets or strings. */
function splitTopLevel(src: string, angle = false): string[] {
  const parts: string[] = [];
  let depth = 0;
  let quote: string | null = null;
  let cur = "";
  for (let i = 0; i < src.length; i++) {
    const ch = src[i] as string;
    if (quote) {
      cur += ch;
      if (ch === "\\") cur += src[++i] ?? "";
      else if (ch === quote) quote = null;
      continue;
    }
    if (!angle && (ch === '"' || ch === "'" || ch === "`")) quote = ch;
    if ("([{".includes(ch) || (angle && ch === "<")) depth++;
    if (")]}".includes(ch) || (angle && ch === ">")) depth--;
    if (ch === "," && depth === 0) {
      parts.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  if (cur.trim()) parts.push(cur.trim());
  return parts.filter(Boolean);
}

function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "))
    .replace(/(^|[^:"'`\\])\/\/[^\n]*/g, (m, p: string) => p + " ".repeat(m.length - p.length));
}

// ── Rust side ────────────────────────────────────────────────────────────────

function rustCommands(): Map<string, RustCommand[]> {
  const byName = new Map<string, RustCommand[]>();
  for (const file of listFiles(RUST_SRC, [".rs"])) {
    const src = readFileSync(file, "utf8").replace(/\/\/[^\n]*/g, "");
    const re =
      /#\[tauri::command(?:\(([^)]*)\))?\]\s*(?:#\[[^\]]*\]\s*)*pub\s+(?:async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(/g;
    for (const m of src.matchAll(re)) {
      const open = (m.index ?? 0) + m[0].length - 1;
      // Rust lifetimes (`State<'_, …>`) are not quotes.
      const close = closingIndex(src, open, "(", ")", '"');
      const params: RustParam[] = [];
      for (const p of splitTopLevel(src.slice(open + 1, close), true)) {
        const pm = /^(?:mut\s+)?(\w+)\s*:\s*([\s\S]+)$/.exec(p);
        if (!pm || INJECTED_TYPE.test(pm[2] as string)) continue;
        params.push({
          name: pm[1] as string,
          optional: /^Option\s*</.test((pm[2] as string).trim()),
          ty: (pm[2] as string).trim(),
        });
      }
      const renameAll = m[1] ? (/rename_all\s*=\s*"([^"]+)"/.exec(m[1])?.[1] ?? null) : null;
      const name = m[2] as string;
      const list = byName.get(name) ?? [];
      list.push({ file: relative(REPO, file).replace(/\\/g, "/"), params, renameAll });
      byName.set(name, list);
    }
  }
  return byName;
}

/** Command name → module path (e.g. `commands/notifications`) from generate_handler!. */
function registeredCommands(): Map<string, string> {
  const lib = readFileSync(join(RUST_SRC, "lib.rs"), "utf8");
  const start = lib.indexOf("generate_handler![");
  const block = lib.slice(start, lib.indexOf("])", start)).replace(/\/\/[^\n]*/g, "");
  const out = new Map<string, string>();
  for (const m of block.matchAll(/((?:\w+::)*)(\w+)\s*,/g)) {
    out.set(m[2] as string, (m[1] as string).replace(/::/g, "/"));
  }
  return out;
}

const toCamel = (s: string): string => s.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
const toSnake = (s: string): string => s.replace(/[A-Z]/g, (c) => `_${c.toLowerCase()}`);

interface RustStruct {
  /** JS-side field names after serde renaming. */
  fields: Set<string>;
  /** Types of `#[serde(flatten)]` fields, whose own fields are merged in. */
  flatten: string[];
}

/** `#[derive(Deserialize)]` structs by name (several modules may reuse a name). */
function rustInputStructs(): Map<string, RustStruct[]> {
  const out = new Map<string, RustStruct[]>();
  for (const file of listFiles(RUST_SRC, [".rs"])) {
    const src = readFileSync(file, "utf8").replace(/\/\/[^\n]*/g, "");
    for (const m of src.matchAll(/((?:#\[[^\]]*\]\s*)*)pub\s+struct\s+(\w+)\s*\{/g)) {
      const attrs = m[1] ?? "";
      if (!/Deserialize/.test(attrs)) continue;
      const renameAll = /serde\([^)]*rename_all\s*=\s*"([^"]+)"/.exec(attrs)?.[1];
      const open = (m.index ?? 0) + m[0].length - 1;
      const body = src.slice(open + 1, closingIndex(src, open, "{", "}", '"'));
      const struct: RustStruct = { fields: new Set(), flatten: [] };
      for (const fm of body.matchAll(/((?:#\[[^\]]*\]\s*)*)pub\s+(?:r#)?(\w+)\s*:\s*([^,\n]+)/g)) {
        const fieldAttrs = fm[1] ?? "";
        const name = fm[2] as string;
        if (/flatten/.test(fieldAttrs)) {
          struct.flatten.push(/(\w+)\s*>*\s*$/.exec(fm[3] as string)?.[1] ?? "");
          continue;
        }
        const rename = /rename\s*=\s*"([^"]+)"/.exec(fieldAttrs)?.[1];
        struct.fields.add(rename ?? (renameAll === "camelCase" ? toCamel(name) : name));
        for (const alias of fieldAttrs.matchAll(/alias\s*=\s*"([^"]+)"/g)) {
          struct.fields.add(alias[1] as string);
        }
      }
      const list = out.get(m[2] as string) ?? [];
      list.push(struct);
      out.set(m[2] as string, list);
    }
  }
  return out;
}

function jsKey(param: string, renameAll: string | null): string {
  return renameAll === "snake_case" ? param : toCamel(param);
}

// ── TypeScript side ──────────────────────────────────────────────────────────

function findCalls(src: string, fnName: string): { index: number; args: string[] }[] {
  const out: { index: number; args: string[] }[] = [];
  for (const m of src.matchAll(new RegExp(`(?<![\\w.$])${fnName}\\s*(<)?`, "g"))) {
    const before = src.slice(Math.max(0, (m.index ?? 0) - 12), m.index);
    if (/function\s*$/.test(before)) continue;
    let i = (m.index ?? 0) + m[0].length;
    if (m[1]) {
      const end = closingIndex(src, i - 1, "<", ">");
      if (end < 0) continue;
      i = end + 1;
    }
    while (/\s/.test(src[i] ?? "")) i++;
    if (src[i] !== "(") continue;
    const close = closingIndex(src, i, "(", ")");
    out.push({ index: m.index ?? 0, args: splitTopLevel(src.slice(i + 1, close)) });
  }
  return out;
}

type ParsedArgs = Pick<InvokeCall, "kind" | "keys" | "spreads" | "values">;

function classifyArgs(expr: string | undefined): ParsedArgs {
  const values = new Map<string, string>();
  if (!expr || expr === "undefined") return { kind: "none", keys: [], spreads: [], values };
  if (!expr.startsWith("{")) return { kind: "expr", keys: [], spreads: [], values };
  const keys: string[] = [];
  const spreads: string[] = [];
  for (const part of splitTopLevel(expr.slice(1, closingIndex(expr, 0, "{", "}")))) {
    if (part.startsWith("...")) {
      spreads.push(part.slice(3).trim());
      continue;
    }
    const km = /^(?:["']([^"']+)["']|([A-Za-z_$][\w$]*))\s*(?::\s*([\s\S]*))?$/.exec(part);
    const key = km ? ((km[1] ?? km[2]) as string) : `<unparsed: ${part.slice(0, 40)}>`;
    keys.push(key);
    values.set(key, km?.[3]?.trim() ?? key);
  }
  return { kind: "literal", keys, spreads, values };
}

/** Parameter name → type name, for the function (declaration or arrow) enclosing `index`. */
function enclosingParamTypes(src: string, index: number): Map<string, string> {
  const before = src.slice(0, index);
  const start = Math.max(before.lastIndexOf("function "), before.lastIndexOf("async ("));
  const types = new Map<string, string>();
  if (start < 0) return types;
  const open = src.indexOf("(", start);
  for (const p of splitTopLevel(src.slice(open + 1, closingIndex(src, open, "(", ")")))) {
    const pm = /^(\w+)\??\s*:\s*([\w.]+)\s*(?:\|\s*(?:null|undefined)\s*)*(?:=.*)?$/.exec(p);
    if (pm) types.set(pm[1] as string, (pm[2] as string).replace(/^.*\./, ""));
  }
  return types;
}

/** Fields of TS interfaces / object type aliases in src/ and shared/, by type name. */
function tsObjectTypes(): Map<string, { fields: string[]; extends: string[] }> {
  const out = new Map<string, { fields: string[]; extends: string[] }>();
  const files = [
    ...listFiles(TS_SRC, [".ts", ".tsx"]),
    ...listFiles(join(REPO, "shared"), [".ts"]),
  ];
  for (const file of files.filter((f) => !/__tests__|\.test\./.test(f))) {
    const src = stripComments(readFileSync(file, "utf8"));
    const re = /(?:interface\s+(\w+)(?:\s+extends\s+([\w\s,<>]+?))?\s*\{|type\s+(\w+)\s*=\s*\{)/g;
    for (const m of src.matchAll(re)) {
      const open = (m.index ?? 0) + m[0].length - 1;
      const body = src.slice(open + 1, closingIndex(src, open, "{", "}"));
      const fields: string[] = [];
      for (const part of splitTopLevel(body.replace(/;/g, ","))) {
        const fm = /^(?:readonly\s+)?["']?(\w+)["']?\??\s*:/.exec(part);
        if (fm) fields.push(fm[1] as string);
      }
      const ext = (m[2] ?? "").split(",").map((e) => e.trim().replace(/<.*/, ""));
      const name = (m[1] ?? m[3]) as string;
      // A name declared twice (e.g. a local type shadowing a shared one) is ambiguous: skip it.
      out.set(name, out.has(name) ? { fields: [], extends: [] } : { fields, extends: ext });
    }
  }
  return out;
}

const STRING_LITERAL = /^(["'`])([^"'`$]+)\1$/;

function invokeCalls(): { calls: InvokeCall[]; dynamic: string[] } {
  const calls: InvokeCall[] = [];
  const dynamic: string[] = [];
  const files = listFiles(TS_SRC, [".ts", ".tsx"]).filter(
    // ipc-invoke.ts defines `invoke` / `invokeSilent` themselves (the central IPC entry).
    (f) => !/__tests__|\.test\.|[\\/]test[\\/]|\.d\.ts$|ipc-invoke\.ts$/.test(f),
  );
  for (const file of files) {
    const src = stripComments(readFileSync(file, "utf8"));
    const rel = relative(REPO, file).replace(/\\/g, "/");
    const lineOf = (idx: number): number => src.slice(0, idx).split("\n").length;

    // Local wrappers: `function w(cmd: string, args…) { … invoke(cmd, args | {…}) }`.
    const wrappers: { name: string; fixed: ParsedArgs | null }[] = [];
    const forwardedNames = new Set<string>();
    const wrapperRe = /function\s+(\w+)\s*(?:<[^>]*>)?\s*\(\s*(\w+)\s*:\s*string\s*,\s*(\w+)/g;
    for (const wm of src.matchAll(wrapperRe)) {
      const bodyStart = src.indexOf("{", src.indexOf(")", (wm.index ?? 0) + wm[0].length));
      const body = src.slice(bodyStart, closingIndex(src, bodyStart, "{", "}"));
      for (const inv of ["invoke", "invokeSilent"]) {
        for (const c of findCalls(body, inv)) {
          if (c.args[0] !== wm[2]) continue;
          forwardedNames.add(wm[2] as string);
          wrappers.push({
            name: wm[1] as string,
            fixed: c.args[1] === wm[3] ? null : classifyArgs(c.args[1]),
          });
        }
      }
    }

    const callers = [
      { name: "invoke", fixed: null },
      { name: "invokeSilent", fixed: null },
      ...wrappers,
    ];
    for (const caller of callers) {
      for (const c of findCalls(src, caller.name)) {
        const where = `${rel}:${lineOf(c.index)}`;
        const lit = STRING_LITERAL.exec(c.args[0] ?? "");
        if (!lit) {
          // Inside a wrapper body the command is the forwarded parameter; its callers are
          // checked through the wrapper. Anything else cannot be checked statically.
          if (!forwardedNames.has(c.args[0] ?? "")) {
            dynamic.push(`${where} ${caller.name}(${c.args[0] ?? ""})`);
          }
          continue;
        }
        calls.push({
          where,
          cmd: lit[2] as string,
          ...(caller.fixed ?? classifyArgs(c.args[1])),
          scope: caller.fixed ? new Map() : enclosingParamTypes(src, c.index),
        });
      }
    }
  }
  return { calls, dynamic };
}

// ── Contract ─────────────────────────────────────────────────────────────────

describe("IPC argument keys", () => {
  const rust = rustCommands();
  const registered = registeredCommands();
  const { calls, dynamic } = invokeCalls();

  function commandFor(cmd: string): RustCommand | undefined {
    const defs = rust.get(cmd);
    if (!defs || defs.length === 0) return undefined;
    const module = registered.get(cmd) ?? "";
    return defs.find((d) => module && d.file.includes(module)) ?? defs[0];
  }

  it("finds the frontend calls and the Rust commands (parser sanity)", () => {
    expect(calls.length).toBeGreaterThan(600);
    expect(rust.size).toBeGreaterThan(800);
    expect(dynamic, "invoke() with a non-literal command name outside a wrapper").toEqual([]);
    // Arguments must be an object literal so their keys can be checked here.
    const opaque = calls.filter((c) => c.kind === "expr").map((c) => `${c.where} ${c.cmd}`);
    expect(opaque, "invoke() whose arguments are not an object literal").toEqual([]);
  });

  it("every invoked command is registered and exists in Rust", () => {
    const missing = calls
      .filter((c) => !registered.has(c.cmd) || !commandFor(c.cmd))
      .map((c) => `${c.where} ${c.cmd}`);
    expect(missing).toEqual([]);
  });

  it("every invoke sends the camelCase keys the Rust command expects", () => {
    const problems: string[] = [];
    for (const call of calls) {
      const def = commandFor(call.cmd);
      if (!def || call.kind === "expr") continue;
      const expected = def.params.map((p) => ({ ...p, key: jsKey(p.name, def.renameAll) }));
      const allowed = new Set(expected.map((e) => e.key));
      const sent = new Set(call.keys);
      const unknown = call.keys.filter((k) => !allowed.has(k));
      const missing =
        call.spreads.length > 0
          ? []
          : expected.filter((e) => !e.optional && !sent.has(e.key)).map((e) => e.key);
      if (unknown.length > 0 || missing.length > 0) {
        problems.push(
          `${call.where} ${call.cmd}: sent {${call.keys.join(", ")}}, ` +
            `expected {${expected.map((e) => e.key + (e.optional ? "?" : "")).join(", ")}}` +
            ` (${def.file})`,
        );
      }
    }
    expect(problems).toEqual([]);
  });

  it("struct arguments use the field casing of the Rust struct", () => {
    const structs = rustInputStructs();
    const tsTypes = tsObjectTypes();
    const tsFields = (name: string, seen = new Set<string>()): string[] => {
      const t = tsTypes.get(name);
      if (!t || seen.has(name)) return [];
      seen.add(name);
      return [...t.fields, ...t.extends.flatMap((e) => tsFields(e, seen))];
    };
    const rustFields = (name: string): Set<string> | null => {
      const defs = structs.get(name);
      if (!defs || defs.length !== 1) return null; // unknown or ambiguous name
      const def = defs[0] as RustStruct;
      const all = new Set(def.fields);
      for (const f of def.flatten) for (const x of rustFields(f) ?? []) all.add(x);
      return all;
    };

    const problems: string[] = [];
    let checked = 0;
    for (const call of calls) {
      const def = commandFor(call.cmd);
      if (!def) continue;
      for (const param of def.params) {
        const value = call.values.get(jsKey(param.name, def.renameAll));
        if (!value) continue;
        const typeName = /(\w+)\s*>*\s*$/.exec(param.ty.replace(/^Option\s*</, ""))?.[1] ?? "";
        const allowed = rustFields(typeName);
        if (!allowed) continue;
        const sent = value.startsWith("{")
          ? classifyArgs(value).keys
          : tsFields(call.scope.get(value) ?? "");
        if (sent.length === 0) continue;
        checked++;
        // Only casing slips (`dateFrom` vs `date_from`): serde ignores such a field silently.
        const wrong = sent.filter(
          (k) => !allowed.has(k) && (allowed.has(toSnake(k)) || allowed.has(toCamel(k))),
        );
        if (wrong.length > 0) {
          problems.push(
            `${call.where} ${call.cmd} ${param.name}: ${typeName} has no {${wrong.join(", ")}}`,
          );
        }
      }
    }
    expect(checked, "struct arguments resolved to a TS type").toBeGreaterThan(300);
    expect(problems).toEqual([]);
  });
});
