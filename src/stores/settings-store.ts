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
import { toErrorMessage } from "@/utils/errors";
import type { PolicySnapshot, PolicyTestResult, SessionPolicy } from "@shared/ipc-types";

interface SettingsState {
  /** Session policy — loaded at startup, defines idle/offline behavior. */
  sessionPolicy: SessionPolicy | null;
  /** True while the policy is being loaded from the backend. */
  policyLoading: boolean;
  /** Error encountered during policy load, or null. Never thrown — Rust falls back to defaults. */
  policyError: string | null;

  /** Currently selected category in the sidebar. */
  activeCategory: string | null;
  /** Policy domain currently being edited (e.g. "session", "backup"), or null. */
  editingPolicy: string | null;

  /** Draft policy snapshot being edited, or null. */
  draftPolicy: PolicySnapshot | null;
  /** Latest test results for the current draft. */
  testResults: PolicyTestResult[] | null;
  /** True while a draft/test/activate operation is in progress. */
  policyOperationLoading: boolean;

  /** Load the active session policy from the backend. Never throws. */
  loadSessionPolicy: () => Promise<void>;
  /** Replace the cached policy after an admin activates a new policy snapshot. */
  applySessionPolicy: (policy: SessionPolicy) => void;
  /** Set the active category in the sidebar. */
  setActiveCategory: (category: string | null) => void;
  /** Open the policy editor for a governed setting domain. */
  openPolicyEditor: (domain: string) => void;
  /** Close the policy editor. */
  closePolicyEditor: () => void;
  /** Store a draft snapshot in the local state. */
  setDraftPolicy: (draft: PolicySnapshot | null) => void;
  /** Store test results from backend validation. */
  setTestResults: (results: PolicyTestResult[] | null) => void;
  /** Set the loading flag for policy operations. */
  setPolicyOperationLoading: (loading: boolean) => void;
}

export const useSettingsStore = create<SettingsState>()((set) => ({
  sessionPolicy: null,
  policyLoading: false,
  policyError: null,
  activeCategory: null,
  editingPolicy: null,
  draftPolicy: null,
  testResults: null,
  policyOperationLoading: false,

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
        policyError: toErrorMessage(err),
      });
    }
  },

  applySessionPolicy: (policy) => set({ sessionPolicy: policy }),

  setActiveCategory: (category) => set({ activeCategory: category }),

  openPolicyEditor: (domain) => set({ editingPolicy: domain }),

  closePolicyEditor: () => set({ editingPolicy: null, draftPolicy: null, testResults: null }),

  setDraftPolicy: (draft) => set({ draftPolicy: draft }),

  setTestResults: (results) => set({ testResults: results }),

  setPolicyOperationLoading: (loading) => set({ policyOperationLoading: loading }),
}));
