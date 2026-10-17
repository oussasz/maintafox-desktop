/**
 * The published equipment lists of Données de référence (EQUIPMENT.STATUS, EQUIPMENT.CRITICALITY,
 * classes, families), loaded once and shared by every equipment chip, filter and form. Screens
 * read a status's meaning from its availability tag and a criticality's rank from the list order
 * — never from codes written in the frontend.
 */

import { useEffect } from "react";
import { create } from "zustand";

import { getEquipmentTaxonomyCatalog } from "@/services/asset-service";
import type { EquipmentTaxonomyCatalog } from "@shared/ipc-types";

interface EquipmentTaxonomyState {
  catalog: EquipmentTaxonomyCatalog | null;
  loading: boolean;
  /** Last load failure (shown by the screens that need the lists), cleared by a success. */
  error: unknown;
  load: () => Promise<void>;
}

export const useEquipmentTaxonomyStore = create<EquipmentTaxonomyState>((set, get) => ({
  catalog: null,
  loading: false,
  error: null,
  load: async () => {
    if (get().loading) return;
    set({ loading: true });
    try {
      const catalog = await getEquipmentTaxonomyCatalog();
      set({ catalog, loading: false, error: null });
    } catch (err) {
      set({ loading: false, error: err });
    }
  },
}));

/** Reload after the lists are edited in Données de référence. */
export function refreshEquipmentTaxonomy(): Promise<void> {
  return useEquipmentTaxonomyStore.getState().load();
}

/** The shared catalog; loads it on first use. */
export function useEquipmentTaxonomy(): {
  catalog: EquipmentTaxonomyCatalog | null;
  error: unknown;
  reload: () => Promise<void>;
} {
  const catalog = useEquipmentTaxonomyStore((s) => s.catalog);
  const error = useEquipmentTaxonomyStore((s) => s.error);
  const load = useEquipmentTaxonomyStore((s) => s.load);
  useEffect(() => {
    const st = useEquipmentTaxonomyStore.getState();
    if (st.catalog == null && !st.loading && st.error == null) void load();
  }, [load]);
  return { catalog, error, reload: load };
}
