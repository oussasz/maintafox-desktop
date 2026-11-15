import { describe, expect, it } from "vitest";

import { summarizeStudy } from "../study/study-summary";

const r = (asset_code: string, mode: string | null, downtime: number | null, included = true) => ({
  included,
  kind: "failure",
  asset_code,
  failure_mode_label: mode,
  downtime_hours: downtime,
});

describe("summarizeStudy", () => {
  it("counts used failures, the most hit machine and mode, and the downtime", () => {
    const s = summarizeStudy([
      r("P-1", "Fuite", 4),
      r("P-1", "Fuite", 2),
      r("P-2", "Usure", null),
      r("P-3", "Usure", 10, false),
      { ...r("P-1", null, 5), kind: "suspension" },
    ]);
    expect(s.failures).toBe(3);
    expect(s.downtimeHours).toBe(6);
    expect(s.topAsset).toEqual({ code: "P-1", count: 2 });
    expect(s.topMode).toEqual({ label: "Fuite", count: 2 });
  });

  it("is empty without used failures", () => {
    expect(summarizeStudy([])).toEqual({
      failures: 0,
      downtimeHours: 0,
      topAsset: null,
      topMode: null,
    });
  });
});
