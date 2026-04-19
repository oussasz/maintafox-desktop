import { describe, it, expect, beforeEach } from "vitest";

import { useAppStore } from "../app-store";

describe("useAppStore", () => {
  beforeEach(() => {
    // Reset store to initial state between tests
    useAppStore.setState({
      appStatus: "loading",
      startupMessage: "",
      appVersion: "",
      isOnline: true,
      syncStatus: {
        state: "idle",
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
      sidebarHoverOpen: false,
      activePath: "/",
    });
  });

  it("has correct initial state", () => {
    const state = useAppStore.getState();
    expect(state.appStatus).toBe("loading");
    expect(state.startupMessage).toBe("");
    expect(state.appVersion).toBe("");
    expect(state.isOnline).toBe(true);
    expect(state.syncStatus.state).toBe("idle");
    expect(state.syncStatus.retryAttempt).toBe(0);
    expect(state.unreadNotificationCount).toBe(0);
    expect(state.hasActiveSession).toBe(false);
    expect(state.currentUserDisplayName).toBeNull();
    expect(state.sidebarCollapsed).toBe(false);
    expect(state.sidebarHoverOpen).toBe(false);
    expect(state.activePath).toBe("/");
  });

  it("setAppStatus updates status and message", () => {
    useAppStore.getState().setAppStatus("ready", "All systems go");
    const state = useAppStore.getState();
    expect(state.appStatus).toBe("ready");
    expect(state.startupMessage).toBe("All systems go");
  });

  it("setAppStatus defaults message to empty string", () => {
    useAppStore.getState().setAppStatus("error");
    const state = useAppStore.getState();
    expect(state.appStatus).toBe("error");
    expect(state.startupMessage).toBe("");
  });

  it("setAppVersion updates version", () => {
    useAppStore.getState().setAppVersion("1.2.3");
    expect(useAppStore.getState().appVersion).toBe("1.2.3");
  });

  it("setOnline updates online status", () => {
    useAppStore.getState().setOnline(false);
    expect(useAppStore.getState().isOnline).toBe(false);
  });

  it("setSyncStatus merges partial sync status", () => {
    useAppStore.getState().setSyncStatus({ state: "running", pendingCount: 5 });
    const { syncStatus } = useAppStore.getState();
    expect(syncStatus.state).toBe("running");
    expect(syncStatus.pendingCount).toBe(5);
    expect(syncStatus.lastSyncAt).toBeNull();
  });

  it("setUnreadNotificationCount updates count", () => {
    useAppStore.getState().setUnreadNotificationCount(42);
    expect(useAppStore.getState().unreadNotificationCount).toBe(42);
  });

  it("setSessionStub updates session state", () => {
    useAppStore.getState().setSessionStub(true, "Oussama");
    const state = useAppStore.getState();
    expect(state.hasActiveSession).toBe(true);
    expect(state.currentUserDisplayName).toBe("Oussama");
  });

  it("toggleSidebar flips collapsed state", () => {
    expect(useAppStore.getState().sidebarCollapsed).toBe(false);
    useAppStore.getState().toggleSidebar();
    expect(useAppStore.getState().sidebarCollapsed).toBe(true);
    useAppStore.getState().toggleSidebar();
    expect(useAppStore.getState().sidebarCollapsed).toBe(false);
  });

  it("setActivePath updates path", () => {
    useAppStore.getState().setActivePath("/equipment");
    expect(useAppStore.getState().activePath).toBe("/equipment");
  });

  it("setSidebarHoverOpen updates hover-open state", () => {
    useAppStore.getState().setSidebarHoverOpen(true);
    expect(useAppStore.getState().sidebarHoverOpen).toBe(true);
    useAppStore.getState().setSidebarHoverOpen(false);
    expect(useAppStore.getState().sidebarHoverOpen).toBe(false);
  });
});
