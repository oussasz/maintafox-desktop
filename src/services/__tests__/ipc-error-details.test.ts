import { beforeEach, describe, expect, it, vi } from "vitest";

import { invoke } from "@/lib/ipc-invoke";
import { extractIpcValidationDetails } from "@/utils/errors";

import { BudgetIpcError, exportPostedActualsForErp } from "../budget-service";
import { InventoryIpcError, listInventoryWarehouses } from "../inventory-service";
import { PersonnelIpcError, listPositions } from "../personnel-service";
import { PlanningIpcError, listScheduleBreakIns } from "../planning-service";
import { PmIpcError, getPmDueMetrics } from "../pm-service";

vi.mock("@/lib/ipc-invoke", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

/** What the backend sends for a refusal (`AppError` serialisation: code, message, details). */
const REFUSAL = {
  code: "VALIDATION_FAILED",
  message: "Week is frozen",
  details: [
    { code: "PLANNING_WEEK_FROZEN", message: "frozen", params: { week_start: "2026-11-02" } },
  ],
};

const CASES: Array<[string, () => Promise<unknown>, new (...a: never[]) => Error]> = [
  ["planning", (): Promise<unknown> => listScheduleBreakIns({} as never), PlanningIpcError],
  ["pm", (): Promise<unknown> => getPmDueMetrics(), PmIpcError],
  ["personnel", (): Promise<unknown> => listPositions(), PersonnelIpcError],
  ["inventory", (): Promise<unknown> => listInventoryWarehouses(), InventoryIpcError],
  ["budget", (): Promise<unknown> => exportPostedActualsForErp(), BudgetIpcError],
];

async function refused(call: () => Promise<unknown>): Promise<unknown> {
  try {
    await call();
  } catch (e) {
    return e;
  }
  throw new Error("the call was not refused");
}

describe("service wrappers keep the refusal details", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  for (const [name, call, cls] of CASES) {
    it(`${name}: a refused call keeps code and details`, async () => {
      mockInvoke.mockRejectedValue(REFUSAL);
      const err = await refused(call);
      expect(err instanceof cls).toBe(true);
      expect((err as { code: string }).code).toBe("VALIDATION_FAILED");
      expect((err as { details: unknown }).details).toEqual(REFUSAL.details);
      expect(extractIpcValidationDetails(err)).toEqual([
        { code: "PLANNING_WEEK_FROZEN", message: "frozen", params: { week_start: "2026-11-02" } },
      ]);
    });

    it(`${name}: a refusal without details has null details`, async () => {
      mockInvoke.mockRejectedValue({ code: "NOT_FOUND", message: "x", details: null });
      const err = await refused(call);
      expect((err as { code: string }).code).toBe("NOT_FOUND");
      expect((err as { details: unknown }).details).toBeNull();
      expect(extractIpcValidationDetails(err)).toEqual([]);
    });
  }
});
