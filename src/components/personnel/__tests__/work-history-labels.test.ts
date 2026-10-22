import { describe, expect, it } from "vitest";

import { toWorkHistoryEntry } from "@/components/personnel/PersonnelDetailWorkspace";
import type { PersonnelWorkHistoryEntry } from "@shared/ipc-types";

// The test translator returns the key, so the assertions read the keys the screen asks for.
const t = (key: string) => key;

function entry(over: Partial<PersonnelWorkHistoryEntry>): PersonnelWorkHistoryEntry {
  return {
    source_module: "di",
    record_id: 7,
    record_code: "DI-0007",
    role_code: "submitter",
    status_code: "submitted",
    happened_at: "2026-10-01T08:00:00Z",
    ...over,
  } as PersonnelWorkHistoryEntry;
}

describe("work history labels", () => {
  it("names a request by its module, role and request status (not raw codes)", () => {
    const e = toWorkHistoryEntry(entry({}), t);
    expect(e.title).toBe("detail.workHistory.module.di DI-0007");
    // `submitted` is the request status shown as "new"; `returned_for_clarification` needs clarification.
    expect(e.subtitle).toBe("detail.workHistory.role.submitter · di:status.new");
    const back = toWorkHistoryEntry(entry({ status_code: "returned_for_clarification" }), t);
    expect(back.subtitle).toContain("di:status.needsClarification");
  });

  it("uses the work-order status vocabulary for a work order", () => {
    const e = toWorkHistoryEntry(
      entry({ source_module: "wo", role_code: "intervener", status_code: "in_progress" }),
      t,
    );
    expect(e.subtitle).toBe("detail.workHistory.role.intervener · ot:status.inProgress");
  });
});
