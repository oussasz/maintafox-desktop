import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n";
import { completeMechanically, WoBlockingError } from "@/services/wo-execution-service";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", () => ipc);

const refusal = {
  code: "VALIDATION_FAILED",
  message: "Validation failed: a; b",
  details: [
    { message: "GATE_CLOSEOUT_FMECA_PARTS_REQUIRED (parts required)" },
    { message: "Validation failed: Tâche obligatoire non terminée" },
  ],
};

describe("a refused completion lists each reason, translated", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("fr");
  });
  beforeEach(() => ipc.invoke.mockReset());

  it("builds the blocking lines from the error's details", async () => {
    ipc.invoke.mockRejectedValueOnce(refusal);
    const err = await completeMechanically({ wo_id: 1 } as never).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(WoBlockingError);
    expect((err as WoBlockingError).blockingErrors).toEqual([
      String(i18n.t("errors:gateError.GATE_CLOSEOUT_FMECA_PARTS_REQUIRED")),
      "Tâche obligatoire non terminée",
    ]);
  });
});
