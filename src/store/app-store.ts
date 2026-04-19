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

export interface AppStore {
  // Startup state
  appStatus: AppStatus;
  startupMessage: string;
  appVersion: string;

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
  sidebarHoverOpen: boolean;
  activePath: string;

  // Actions
  setAppStatus: (status: AppStatus, message?: string) => void;
  setAppVersion: (version: string) => void;
  setOnline: (online: boolean) => void;
  setSyncStatus: (s: Partial<SyncStatus>) => void;
  setUnreadNotificationCount: (n: number) => void;
  setSessionStub: (hasSession: boolean, displayName: string | null) => void;
  toggleSidebar: () => void;
  setSidebarCollapsed: (collapsed: boolean) => void;
  setSidebarHoverOpen: (open: boolean) => void;
  setActivePath: (path: string) => void;
}

export const useAppStore = create<AppStore>()(
  persist(
    (set) => ({
      appStatus: "loading" as AppStatus,
      startupMessage: "",
      appVersion: "",
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
      sidebarCollapsed: true,
      sidebarHoverOpen: false,
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
      setSidebarCollapsed: (sidebarCollapsed): void => {
        set({ sidebarCollapsed });
      },
      setSidebarHoverOpen: (sidebarHoverOpen): void => {
        set({ sidebarHoverOpen });
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
