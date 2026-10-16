/**
 * di-stats-store — behaviour tests.
 *
 * Covers filter merging, the loadStats loading/error lifecycle and the exact
 * filter forwarded to the DI stats service.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { getDiStats } from "@/services/di-stats-service";
import type { DiStatsPayload } from "@shared/ipc-types";

import { useDiStatsStore } from "../di-stats-store";

vi.mock("@/services/di-stats-service", () => ({
  getDiStats: vi.fn(),
}));

const mockGetDiStats = vi.mocked(getDiStats);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeStats(overrides: Partial<DiStatsPayload> = {}): DiStatsPayload {
  return {
    total: 12,
    pending: 3,
    in_progress: 4,
    closed: 5,
    closed_this_month: 2,
    overdue: 1,
    sla_met_count: 4,
    sla_breached_count: 1,
    sla_total: 5,
    safety_issues: 0,
    status_distribution: [{ status: "submitted", count: 3 }],
    priority_distribution: [{ priority: "high", count: 2 }],
    type_distribution: [{ origin_type: "operator", count: 12 }],
    disposition_distribution: [{ disposition_code: "repair", count: 5 }],
    monthly_trend: [{ period: "2026-09", created: 6, closed: 3 }],
    available_years: [2025, 2026],
    avg_age_days: 4.5,
    max_age_days: 30,
    top_equipment: [{ equipment_id: 10, asset_label: "P-101", count: 4, percentage: 33.3 }],
    overdue_dis: [{ id: 7, code: "DI-0007", title: "Fuite", priority: "high", hours_overdue: 72 }],
    backlog_by_status: [{ status: "submitted", count: 3, avg_age_days: 2, max_age_days: 6 }],
    sla_open: { on_track: 2, at_risk: 1, breached: 1, no_target: 0 },
    converted_count: 3,
    avg_response_hours: 5.5,
    avg_conversion_hours: 40,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const initial = useDiStatsStore.getState();

function resetStore() {
  useDiStatsStore.setState(initial, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockGetDiStats.mockReset();
  resetStore();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("di-stats-store", () => {
  it("starts with no stats, an empty filter and idle flags", () => {
    const s = useDiStatsStore.getState();
    expect(s.stats).toBeNull();
    expect(s.filter).toEqual({});
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });

  describe("setFilter", () => {
    it("merges partial patches into the existing filter", () => {
      useDiStatsStore.getState().setFilter({ date_from: "2026-01-01" });
      useDiStatsStore.getState().setFilter({ entity_id: 5 });
      expect(useDiStatsStore.getState().filter).toEqual({
        date_from: "2026-01-01",
        entity_id: 5,
      });
    });

    it("overwrites existing keys and allows clearing them with null", () => {
      useDiStatsStore.getState().setFilter({ date_to: "2026-02-01", entity_id: 5 });
      useDiStatsStore.getState().setFilter({ date_to: "2026-03-01", entity_id: null });
      expect(useDiStatsStore.getState().filter).toEqual({
        date_to: "2026-03-01",
        entity_id: null,
      });
    });

    it("does not trigger a load by itself", () => {
      useDiStatsStore.getState().setFilter({ entity_id: 1 });
      expect(mockGetDiStats).not.toHaveBeenCalled();
    });
  });

  describe("loadStats", () => {
    it("passes the current filter to the service and stores the payload", async () => {
      const stats = makeStats();
      mockGetDiStats.mockResolvedValueOnce(stats);
      useDiStatsStore.getState().setFilter({ date_from: "2026-01-01", entity_id: 3 });

      await useDiStatsStore.getState().loadStats();

      expect(mockGetDiStats).toHaveBeenCalledTimes(1);
      expect(mockGetDiStats).toHaveBeenCalledWith({ date_from: "2026-01-01", entity_id: 3 });
      const s = useDiStatsStore.getState();
      expect(s.stats).toEqual(stats);
      expect(s.error).toBeNull();
      expect(s.loading).toBe(false);
    });

    it("sets loading=true while in flight and clears a previous error", async () => {
      const d = deferred<DiStatsPayload>();
      mockGetDiStats.mockReturnValueOnce(d.promise);
      useDiStatsStore.setState({ error: "old" });

      const p = useDiStatsStore.getState().loadStats();
      expect(useDiStatsStore.getState().loading).toBe(true);
      expect(useDiStatsStore.getState().error).toBeNull();

      d.resolve(makeStats({ total: 99 }));
      await p;
      expect(useDiStatsStore.getState().loading).toBe(false);
      expect(useDiStatsStore.getState().stats?.total).toBe(99);
    });

    it("sets error from an IPC error object, swallows it and keeps previous stats", async () => {
      const previous = makeStats({ total: 1 });
      useDiStatsStore.setState({ stats: previous });
      mockGetDiStats.mockRejectedValueOnce({ code: "DB_ERROR", message: "db locked" });

      await expect(useDiStatsStore.getState().loadStats()).resolves.toBeUndefined();

      const s = useDiStatsStore.getState();
      expect(s.error).toBe("db locked");
      expect(s.loading).toBe(false);
      expect(s.stats).toBe(previous);
    });

    it("uses Error.message when the service throws an Error", async () => {
      mockGetDiStats.mockRejectedValueOnce(new Error("boom"));
      await useDiStatsStore.getState().loadStats();
      expect(useDiStatsStore.getState().error).toBe("boom");
    });
  });
});
