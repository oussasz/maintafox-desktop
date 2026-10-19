/**
 * The screens check a text's length the way the server does (trimmed, in Unicode code points) and
 * repeat the server's minimums. The server does not send those minimums to the screens, so this
 * test reads them from the Rust source: if a minimum changes there and not here, it fails.
 */
import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import { HANDOVER_NOTE_MIN } from "../PermitHandoverForm";
import { ISOLATION_REASON_MIN, THREE_PERSON_REASON_MIN } from "../PermitTypesEditor";
import { serverTextLength, serverTrim } from "../permit-rules";

function rustConstant(file: string, name: string): number {
  const source = readFileSync(file, "utf8");
  const match = new RegExp(`pub const ${name}: usize = ([0-9]+);`).exec(source);
  if (!match) throw new Error(`${name} not found in ${file}`);
  return Number(match[1]);
}

describe("minimum text lengths follow the server", () => {
  it("hand-over note: same minimum as MIN_HANDOVER_NOTE_CHARS", () => {
    expect(HANDOVER_NOTE_MIN).toBe(
      rustConstant("src-tauri/src/permit/domain.rs", "MIN_HANDOVER_NOTE_CHARS"),
    );
  });

  it("three-person reason: same minimum as MIN_REASON_CHARS", () => {
    expect(THREE_PERSON_REASON_MIN).toBe(
      rustConstant("src-tauri/src/settings/catalog.rs", "MIN_REASON_CHARS"),
    );
  });

  it("energy-isolation switch-off reason: same minimum as MIN_REASON_CHARS", () => {
    expect(ISOLATION_REASON_MIN).toBe(
      rustConstant("src-tauri/src/settings/catalog.rs", "MIN_REASON_CHARS"),
    );
    // The server checks this reason against that constant (guard_isolation_relax).
    const queries = readFileSync("src-tauri/src/permit/queries.rs", "utf8");
    const guard = /async fn guard_isolation_relax[\s\S]*?\r?\n\}\r?\n/.exec(queries)?.[0];
    expect(guard).toContain("let min = crate::settings::catalog::MIN_REASON_CHARS;");
  });
});

describe("serverTextLength", () => {
  it("ignores the spaces around the text", () => {
    expect(serverTextLength("   abc  \n")).toBe(3);
    expect(serverTrim("\u3000 abc \u00a0")).toBe("abc");
  });

  it("counts a character outside the first plane once, like the server", () => {
    // "𝒜" and the emoji are two UTF-16 units but one Rust char.
    expect("𝒜".length).toBe(2);
    expect(serverTextLength("𝒜")).toBe(1);
    expect(serverTextLength("ab😀")).toBe(3);
  });

  it("trims what Rust trims (U+0085) and keeps what Rust keeps (U+FEFF)", () => {
    expect(serverTextLength("\u0085ab\u0085")).toBe(2);
    expect(serverTextLength("\ufeffab")).toBe(3);
  });
});
