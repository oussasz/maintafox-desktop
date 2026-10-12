import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

import {
  WO_LIFECYCLE_STATES,
  type WoLifecycleFacts,
  deriveWoLifecycle,
  isWoLifecycleState,
} from "../wo-lifecycle";

interface Case extends WoLifecycleFacts {
  state: string;
}

// Written by the Rust test `wo::lifecycle::tests::shared_cases_file_matches_derive`.
const cases = JSON.parse(
  readFileSync(resolve(__dirname, "../../../shared/wo/lifecycle-cases.json"), "utf8"),
) as Case[];

describe("deriveWoLifecycle (mirror of Rust wo::lifecycle::derive)", () => {
  it("agrees with Rust on every combination of facts", () => {
    expect(cases).toHaveLength(768);
    for (const c of cases) {
      expect({ facts: c, state: deriveWoLifecycle(c) }).toEqual({ facts: c, state: c.state });
    }
  });

  it("reaches all twelve PRD states", () => {
    const reached = new Set(cases.map((c) => deriveWoLifecycle(c)));
    expect([...reached].sort()).toEqual([...WO_LIFECYCLE_STATES].sort());
  });

  it("recognises state codes", () => {
    expect(isWoLifecycleState("waiting_for_prerequisite")).toBe(true);
    expect(isWoLifecycleState("on_hold")).toBe(false);
    expect(isWoLifecycleState(null)).toBe(false);
  });
});
