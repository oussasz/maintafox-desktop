import { create } from "zustand";
import { persist } from "zustand/middleware";

import type { AppStatus } from "@/types";

export interface SyncStatus {
  state: "idle" | "scheduled" | "running" | "blocked" | "degraded" | "error" | "paused";
  pendingCount: number;
  lastSyncAt: string | null;
  errorMessage: string | null;
  blockerReason?: string | null;
  retryAttempt?: number;
}

/** A failed database update after an in-app update (AppStatus "update_rollback_required"). */
export interface UpdateRecoveryInfo {
  /** The version to reinstall (the one this device ran before the update). */
  fromVersion: string;
  /** The version that could not update the database (the one running now). */
  toVersion: string;
  /** Technical reason (migration error). */
  reason: string;
}

export interface AppStore {
  // Startup state
  appStatus: AppStatus;
  startupMessage: string;
  appVersion: string;
  updateRecovery: UpdateRecoveryInfo | null;

  // Sync and connectivity
  isOnline: boolean;
  syncStatus: SyncStatus;

  // Notification badge
  unreadNotificationCount: number;

  // Session (Phase 1 stub — replaced in Sub-phase 04)
  hasActiveSession: boolean;
  currentUserDisplayName: string | null;

  // UI: sidebar
  sidebarCollapsed: boolean;
  activePath: string;

  // Actions
  setAppStatus: (status: AppStatus, message?: string) => void;
  setAppVersion: (version: string) => void;
  /** Shows the update recovery screen (AppStatus "update_rollback_required"). */
  setUpdateRecovery: (info: UpdateRecoveryInfo) => void;
  setOnline: (online: boolean) => void;
  setSyncStatus: (s: Partial<SyncStatus>) => void;
  setUnreadNotificationCount: (n: number) => void;
  setSessionStub: (hasSession: boolean, displayName: string | null) => void;
  toggleSidebar: () => void;
  setActivePath: (path: string) => void;
}

export const useAppStore = create<AppStore>()(
  persist(
    (set) => ({
      appStatus: "loading" as AppStatus,
      startupMessage: "",
      appVersion: "",
      updateRecovery: null,
      isOnline: true,
      syncStatus: {
        state: "idle" as const,
        pendingCount: 0,
        lastSyncAt: null,
        errorMessage: null,
        blockerReason: null,
        retryAttempt: 0,
      },
      unreadNotificationCount: 0,
      hasActiveSession: false,
      currentUserDisplayName: null,
      sidebarCollapsed: false,
      activePath: "/",

      setAppStatus: (appStatus, startupMessage): void => {
        set({
          appStatus,
          startupMessage: startupMessage ?? "",
        });
      },
      setAppVersion: (appVersion): void => {
        set({ appVersion });
      },
      setUpdateRecovery: (updateRecovery): void => {
        set({ updateRecovery, appStatus: "update_rollback_required", startupMessage: "" });
      },
      setOnline: (isOnline): void => {
        set({ isOnline });
      },
      setSyncStatus: (s): void => {
        set((st) => ({ syncStatus: { ...st.syncStatus, ...s } }));
      },
      setUnreadNotificationCount: (unreadNotificationCount): void => {
        set({ unreadNotificationCount });
      },
      setSessionStub: (hasActiveSession, currentUserDisplayName): void => {
        set({ hasActiveSession, currentUserDisplayName });
      },
      toggleSidebar: (): void => {
        set((st) => ({ sidebarCollapsed: !st.sidebarCollapsed }));
      },
      setActivePath: (activePath): void => {
        set({ activePath });
      },
    }),
    {
      name: "maintafox-app",
      partialize: (s) => ({
        sidebarCollapsed: s.sidebarCollapsed,
      }),
    },
  ),
);
