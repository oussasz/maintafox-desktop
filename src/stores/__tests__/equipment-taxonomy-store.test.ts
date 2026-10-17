import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  refreshEquipmentTaxonomy,
  useEquipmentTaxonomy,
  useEquipmentTaxonomyStore,
} from "@/stores/equipment-taxonomy-store";
import type { EquipmentTaxonomyCatalog } from "@shared/ipc-types";

const svc = vi.hoisted(() => ({ getEquipmentTaxonomyCatalog: vi.fn() }));
vi.mock("@/services/asset-service", () => svc);

const CATALOG: EquipmentTaxonomyCatalog = {
  statuses: [
    {
      id: 1,
      code: "ACTIVE_IN_SERVICE",
      label: "En service",
      parent_id: null,
      color_hex: null,
      semantic_tag: "up",
      is_system: true,
    },
  ],
  criticalities: [],
  classes: [],
  families: [],
  subfamilies: [],
};

beforeEach(() => {
  vi.clearAllMocks();
  useEquipmentTaxonomyStore.setState({ catalog: null, loading: false, error: null });
});

describe("equipment taxonomy store", () => {
  it("loads the published lists once for every consumer", async () => {
    svc.getEquipmentTaxonomyCatalog.mockResolvedValue(CATALOG);
    const a = renderHook(() => useEquipmentTaxonomy());
    renderHook(() => useEquipmentTaxonomy());
    await waitFor(() => expect(a.result.current.catalog).toEqual(CATALOG));
    expect(svc.getEquipmentTaxonomyCatalog).toHaveBeenCalledTimes(1);
  });

  it("keeps the failure for the screens and does not retry by itself", async () => {
    svc.getEquipmentTaxonomyCatalog.mockRejectedValueOnce({ code: "DATABASE_ERROR" });
    const { result, rerender } = renderHook(() => useEquipmentTaxonomy());
    await waitFor(() => expect(result.current.error).toEqual({ code: "DATABASE_ERROR" }));
    rerender();
    expect(svc.getEquipmentTaxonomyCatalog).toHaveBeenCalledTimes(1);

    svc.getEquipmentTaxonomyCatalog.mockResolvedValueOnce(CATALOG);
    await act(async () => {
      await result.current.reload();
    });
    expect(result.current.catalog).toEqual(CATALOG);
    expect(result.current.error).toBeNull();
  });

  it("ignores a second load while one runs, and refreshes on demand", async () => {
    let resolve: (c: EquipmentTaxonomyCatalog) => void = () => undefined;
    svc.getEquipmentTaxonomyCatalog.mockImplementationOnce(
      () => new Promise<EquipmentTaxonomyCatalog>((r) => (resolve = r)),
    );
    const first = refreshEquipmentTaxonomy();
    await refreshEquipmentTaxonomy();
    expect(svc.getEquipmentTaxonomyCatalog).toHaveBeenCalledTimes(1);
    resolve(CATALOG);
    await first;
    expect(useEquipmentTaxonomyStore.getState().catalog).toEqual(CATALOG);
  });
});
