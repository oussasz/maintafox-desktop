// src/stores/settings-store.ts
//
// Zustand store for the settings control plane. Acts as a read-through cache:
//   - `sessionPolicy` is loaded once at startup and refreshed on policy activation.
//   - Individual setting values are NOT cached here; they are fetched on demand
//     by Settings UI components via settingsService.getSetting().
//
// This store has NO write methods — settings writes go directly through
// settingsService.setSetting() followed by a targeted refetch.

import { create } from "zustand";

import { getSessionPolicy } from "@/services/settings-service";
import type { SessionPolicy } from "@shared/ipc-types";

interface SettingsState {
  /** Session policy — loaded at startup, defines idle/offline behavior. */
  sessionPolicy: SessionPolicy | null;
  /** True while the policy is being loaded from the backend. */
  policyLoading: boolean;
  /** Error encountered during policy load, or null. Never thrown — Rust falls back to defaults. */
  policyError: string | null;

  /** Load the active session policy from the backend. Never throws. */
  loadSessionPolicy: () => Promise<void>;
  /** Replace the cached policy after an admin activates a new policy snapshot. */
  applySessionPolicy: (policy: SessionPolicy) => void;
}

export const useSettingsStore = create<SettingsState>()((set) => ({
  sessionPolicy: null,
  policyLoading: false,
  policyError: null,

  loadSessionPolicy: async () => {
    set({ policyLoading: true, policyError: null });
    try {
      const policy = await getSessionPolicy();
      set({ sessionPolicy: policy, policyLoading: false });
    } catch (err) {
      // The Rust command never returns an error — it always falls back to defaults.
      // This catch-path handles the case where the Tauri bridge is unavailable (dev/test).
      console.warn("settings-store: failed to load session policy", err);
      set({
        policyLoading: false,
        policyError: err instanceof Error ? err.message : String(err),
      });
    }
  },

  applySessionPolicy: (policy) => set({ sessionPolicy: policy }),
}));
