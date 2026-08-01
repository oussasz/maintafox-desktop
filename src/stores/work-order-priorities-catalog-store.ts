/**
 * Shared catalog of work order priorities (urgency_levels) for forms and filters.
 */

import { create } from "zustand";

import { listWorkOrderPriorities } from "@/services/wo-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderPriorityOption } from "@shared/ipc-types";

interface State {
  priorities: WorkOrderPriorityOption[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
}

export const useWorkOrderPrioritiesCatalog = create<State>((set) => ({
  priorities: [],
  loading: false,
  error: null,
  load: async () => {
    set({ loading: true, error: null });
    try {
      const priorities = await listWorkOrderPriorities();
      set({ priorities, loading: false });
    } catch (err) {
      set({ loading: false, error: toErrorMessage(err) });
    }
  },
}));

export function refreshWorkOrderPrioritiesCatalog(): Promise<void> {
  return useWorkOrderPrioritiesCatalog.getState().load();
}
