import { describe, expect, it } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import frTraining from "@/i18n/locale-data/fr/training.json";
import { GAP_STATUSES, MATRIX_CELL_STATUSES } from "@/services/qualification-gate-service";

import {
  GAP_STATUS_TONE,
  MATRIX_CELL_TONE,
  addMonths,
  daysUntil,
  todayIso,
} from "../qualification-tones";

function keys(obj: unknown, prefix = ""): string[] {
  if (obj == null || typeof obj !== "object") return [prefix];
  return Object.entries(obj as Record<string, unknown>).flatMap(([k, v]) =>
    keys(v, prefix ? `${prefix}.${k}` : k),
  );
}

describe("qualification dates", () => {
  it("adds calendar months and clamps to the month end", () => {
    expect(addMonths("2026-01-31", 1)).toBe("2026-02-28");
    expect(addMonths("2024-01-31", 1)).toBe("2024-02-29");
    expect(addMonths("2026-10-14", 36)).toBe("2029-10-14");
    expect(addMonths("not a date", 3)).toBeNull();
  });

  it("counts days left, negative once expired", () => {
    expect(daysUntil("2026-10-24", "2026-10-14")).toBe(10);
    expect(daysUntil("2026-10-10T00:00:00Z", "2026-10-14")).toBe(-4);
    expect(daysUntil("??", "2026-10-14")).toBeNull();
  });

  it("formats today as YYYY-MM-DD", () => {
    expect(todayIso(new Date(2026, 0, 5))).toBe("2026-01-05");
  });
});

describe("qualification tones", () => {
  it("gives every matrix and gap state a tone, orange only for what runs out", () => {
    for (const s of MATRIX_CELL_STATUSES) expect(MATRIX_CELL_TONE[s]).toBeDefined();
    for (const s of GAP_STATUSES) expect(GAP_STATUS_TONE[s]).toBeDefined();
    expect(MATRIX_CELL_TONE.expiring_soon).toBe("attention");
    expect(MATRIX_CELL_TONE.expired).toBe("danger");
    expect(MATRIX_CELL_TONE.valid).toBe("success");
  });
});

describe("training translations", () => {
  it("has the same keys in French and English", () => {
    expect(keys(frTraining).sort()).toEqual(keys(enTraining).sort());
  });

  it("translates every gap status and matrix state", () => {
    for (const s of GAP_STATUSES) {
      expect(enTraining.gapStatus[s]).toBeTruthy();
      expect(frTraining.gap[s]).toBeTruthy();
    }
    for (const s of MATRIX_CELL_STATUSES) expect(frTraining.cellStatus[s]).toBeTruthy();
  });
});
