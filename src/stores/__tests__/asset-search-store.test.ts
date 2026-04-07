/**
 * Supervisor Verification — Phase 2 - SP02 - F03 - Sprint S1
 *
 * V1 — Code-priority search: searching exact asset code returns
 *       target row first (ranking verified via mock ordering contract).
 * V2 — Multi-filter search: class + status filters are forwarded
 *       correctly and results are constrained to the filter set.
 * V3 — Empty-state behavior: nonsense query yields zero results
 *       without error; store stays in a clean state.
 */

import { describe, it, expect, beforeEach } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";
import type { AssetSearchResult } from "@shared/ipc-types";

import { useAssetSearchStore } from "../asset-search-store";

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeResult(overrides: Partial<AssetSearchResult> = {}): AssetSearchResult {
  return {
    id: 1,
    sync_id: "aaa-111",
    asset_code: "PMP-001",
    asset_name: "Pompe Principale",
    class_code: "PUMP",
    class_name: "Pompe",
    family_code: "ROTATING",
    family_name: "Équipement Rotatif",
    criticality_code: "HIGH",
    status_code: "ACTIVE",
    org_node_id: 1,
    org_node_name: "Usine Principale",
    parent_asset_id: null,
    parent_asset_code: null,
    parent_asset_name: null,
    primary_meter_name: "Heures de marche",
    primary_meter_reading: 12450,
    primary_meter_unit: "h",
    primary_meter_last_read_at: "2026-04-01T08:00:00Z",
    external_id_count: 2,
    row_version: 1,
    ...overrides,
  };
}

const pmp001 = makeResult({ id: 1, asset_code: "PMP-001", asset_name: "Pompe Principale" });
const pmp002 = makeResult({
  id: 2,
  sync_id: "bbb-222",
  asset_code: "PMP-002",
  asset_name: "Pompe Secondaire",
  status_code: "STANDBY",
  row_version: 1,
});
const vlv001 = makeResult({
  id: 3,
  sync_id: "ccc-333",
  asset_code: "VLV-001",
  asset_name: "Vanne d'Isolement",
  class_code: "VALVE",
  class_name: "Vanne",
  family_code: "STATIC",
  family_name: "Équipement Statique",
  status_code: "ACTIVE",
  row_version: 1,
});

function resetStore() {
  useAssetSearchStore.setState({
    filters: {
      query: null,
      classCodes: null,
      familyCodes: null,
      statusCodes: null,
      orgNodeIds: null,
      includeDecommissioned: false,
      limit: 100,
    },
    results: [],
    selectedResultId: null,
    loading: false,
    error: null,
  });
}

// ── V1 — Code-priority search ────────────────────────────────────────────────

describe("V1 — Code-priority search", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("searching exact asset code returns target row first", async () => {
    // Backend contract: exact code match is ranked first (ORDER BY CASE 0)
    // Mock returns exact-match row first, then partial-match rows.
    mockInvoke.mockResolvedValueOnce([pmp001, pmp002, vlv001]);

    await useAssetSearchStore.getState().updateFilters({ query: "PMP-001" });

    expect(mockInvoke).toHaveBeenCalledWith("search_assets", {
      filters: {
        query: "PMP-001",
        classCodes: null,
        familyCodes: null,
        statusCodes: null,
        orgNodeIds: null,
        includeDecommissioned: false,
        limit: 100,
      },
    });

    const state = useAssetSearchStore.getState();
    expect(state.error).toBeNull();
    expect(state.results).toHaveLength(3);
    // Target row (exact code match) is first
    expect(state.results[0]?.asset_code).toBe("PMP-001");
    expect(state.loading).toBe(false);
  });

  it("query filter is passed through to IPC invoke", async () => {
    mockInvoke.mockResolvedValueOnce([pmp001]);

    await useAssetSearchStore.getState().updateFilters({ query: "PMP-001" });

    const callArgs = mockInvoke.mock.calls[0];
    expect(callArgs?.[0]).toBe("search_assets");
    expect((callArgs?.[1] as { filters: { query: string } }).filters.query).toBe("PMP-001");
  });
});

// ── V2 — Multi-filter search ─────────────────────────────────────────────────

describe("V2 — Multi-filter search", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("class + status filters are forwarded to IPC command", async () => {
    // Only PUMP + ACTIVE → only pmp001 returned
    mockInvoke.mockResolvedValueOnce([pmp001]);

    await useAssetSearchStore.getState().updateFilters({
      classCodes: ["PUMP"],
      statusCodes: ["ACTIVE"],
    });

    expect(mockInvoke).toHaveBeenCalledWith("search_assets", {
      filters: {
        query: null,
        classCodes: ["PUMP"],
        statusCodes: ["ACTIVE"],
        familyCodes: null,
        orgNodeIds: null,
        includeDecommissioned: false,
        limit: 100,
      },
    });

    const state = useAssetSearchStore.getState();
    expect(state.results).toHaveLength(1);
    expect(state.results[0]?.class_code).toBe("PUMP");
    expect(state.results[0]?.status_code).toBe("ACTIVE");
    expect(state.error).toBeNull();
  });

  it("combining query + class + status constrains results", async () => {
    mockInvoke.mockResolvedValueOnce([pmp001]);

    await useAssetSearchStore.getState().updateFilters({
      query: "PMP",
      classCodes: ["PUMP"],
      statusCodes: ["ACTIVE"],
    });

    expect(mockInvoke).toHaveBeenCalledWith("search_assets", {
      filters: {
        query: "PMP",
        classCodes: ["PUMP"],
        statusCodes: ["ACTIVE"],
        familyCodes: null,
        orgNodeIds: null,
        includeDecommissioned: false,
        limit: 100,
      },
    });

    const { results } = useAssetSearchStore.getState();
    expect(results).toHaveLength(1);
    expect(results[0]?.asset_code).toBe("PMP-001");
  });

  it("clearFilters resets to defaults and re-searches", async () => {
    // First: apply filters
    mockInvoke.mockResolvedValueOnce([pmp001]);
    await useAssetSearchStore.getState().updateFilters({
      classCodes: ["PUMP"],
      statusCodes: ["ACTIVE"],
    });

    // Then: clear
    mockInvoke.mockResolvedValueOnce([pmp001, pmp002, vlv001]);
    await useAssetSearchStore.getState().clearFilters();

    const state = useAssetSearchStore.getState();
    expect(state.filters.classCodes).toBeNull();
    expect(state.filters.statusCodes).toBeNull();
    expect(state.filters.query).toBeNull();
    expect(state.results).toHaveLength(3);
    expect(state.selectedResultId).toBeNull();
    expect(state.error).toBeNull();
  });
});

// ── V3 — Empty-state behavior ────────────────────────────────────────────────

describe("V3 — Empty-state behavior", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    resetStore();
  });

  it("nonsense query returns empty results without error", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    await useAssetSearchStore.getState().updateFilters({ query: "ZZZXXX_NONEXISTENT_999" });

    const state = useAssetSearchStore.getState();
    expect(state.results).toEqual([]);
    expect(state.error).toBeNull();
    expect(state.loading).toBe(false);
  });

  it("empty array propagates through Zod validation", async () => {
    mockInvoke.mockResolvedValueOnce([]);

    await useAssetSearchStore.getState().runSearch();

    const state = useAssetSearchStore.getState();
    expect(state.results).toEqual([]);
    expect(state.error).toBeNull();
  });

  it("backend error sets error state and clears results", async () => {
    // First: load some results
    mockInvoke.mockResolvedValueOnce([pmp001, pmp002]);
    await useAssetSearchStore.getState().runSearch();
    expect(useAssetSearchStore.getState().results).toHaveLength(2);

    // Then: backend failure
    mockInvoke.mockRejectedValueOnce(new Error("Database connection lost"));
    await useAssetSearchStore.getState().runSearch();

    const state = useAssetSearchStore.getState();
    expect(state.results).toEqual([]);
    expect(state.error).toBe("Database connection lost");
    expect(state.loading).toBe(false);
  });

  it("selectAsset works independently from search results", () => {
    useAssetSearchStore.getState().selectAsset(42);
    expect(useAssetSearchStore.getState().selectedResultId).toBe(42);

    useAssetSearchStore.getState().selectAsset(null);
    expect(useAssetSearchStore.getState().selectedResultId).toBeNull();
  });
});
