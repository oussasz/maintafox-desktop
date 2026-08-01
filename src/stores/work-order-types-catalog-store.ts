/**
 * Shared catalog of work order types for dropdowns and filters across the app.
 * Refresh after edits in Reference Data (Données de référence) so all modules stay aligned.
 */

import { create } from "zustand";

import { listWorkOrderTypes } from "@/services/wo-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderTypeOption } from "@shared/ipc-types";

interface WorkOrderTypesCatalogState {
  types: WorkOrderTypeOption[];
  loading: boolean;
  error: string | null;
  /** Loads from the backend and updates subscribers. */
  load: () => Promise<void>;
}

export const useWorkOrderTypesCatalog = create<WorkOrderTypesCatalogState>((set) => ({
  types: [],
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const types = await listWorkOrderTypes();
      set({ types, loading: false });
    } catch (err) {
      set({ loading: false, error: toErrorMessage(err) });
    }
  },
}));

/** Call after mutating types in Reference Manager (or anywhere) to refresh all consumers. */
export function refreshWorkOrderTypesCatalog(): Promise<void> {
  return useWorkOrderTypesCatalog.getState().load();
}
