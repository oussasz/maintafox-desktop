import { Menu, Bell, RefreshCw, AlertCircle, User } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useAppStore } from "@/store/app-store";

export function TopBar() {
  const { t } = useTranslation("shell");
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);
  const syncStatus = useAppStore((s) => s.syncStatus);
  const unreadCount = useAppStore((s) => s.unreadNotificationCount);
  const isOnline = useAppStore((s) => s.isOnline);
  const displayName = useAppStore((s) => s.currentUserDisplayName);

  return (
    <header
      className="fixed inset-x-0 top-0 z-30 flex h-topbar items-center
                 border-b border-surface-border bg-surface-1 px-3 gap-2"
      data-tauri-drag-region
    >
      {/* Sidebar toggle */}
      <button
        onClick={toggleSidebar}
        aria-label={t("sidebar.toggle")}
        className="btn-ghost px-2 py-1.5"
      >
        <Menu className="h-4 w-4" />
      </button>

      {/* Logo / product name */}
      <span className="text-sm font-semibold text-text-primary select-none mr-4">Maintafox</span>

      {/* Search placeholder — populated in Phase 2 */}
      <div className="flex-1 hidden md:flex items-center">
        <div
          className="flex items-center gap-2 rounded-md border border-surface-border
                     bg-surface-2 px-3 py-1 text-sm text-text-muted cursor-pointer
                     hover:border-primary-light transition-colors duration-fast w-72"
        >
          <span>⌘K</span>
          <span>{t("search.placeholder")}</span>
        </div>
      </div>

      {/* Right controls */}
      <div className="ml-auto flex items-center gap-1">
        {/* Sync status indicator */}
        <SyncIndicator state={syncStatus.state} isOnline={isOnline} />

        {/* Notification bell */}
        <button
          aria-label={t("notifications.label", { count: unreadCount })}
          className="relative btn-ghost px-2 py-1.5"
        >
          <Bell className="h-4 w-4" />
          {unreadCount > 0 && (
            <span
              className="absolute -right-0.5 -top-0.5 flex h-4 w-4 items-center
                         justify-center rounded-full bg-status-danger
                         text-2xs font-bold text-white"
            >
              {unreadCount > 99 ? "99+" : unreadCount}
            </span>
          )}
        </button>

        {/* User menu trigger */}
        <button
          aria-label={displayName ?? t("user.menu")}
          className="btn-ghost flex items-center gap-2 px-2 py-1.5"
        >
          <div
            className="flex h-6 w-6 items-center justify-center
                       rounded-full bg-primary text-xs font-semibold text-white"
          >
            {displayName ? displayName.charAt(0).toUpperCase() : <User className="h-3.5 w-3.5" />}
          </div>
          {displayName && (
            <span className="hidden lg:inline text-sm text-text-secondary max-w-32 truncate">
              {displayName}
            </span>
          )}
        </button>
      </div>
    </header>
  );
}

function SyncIndicator({ state, isOnline }: { state: string; isOnline: boolean }) {
  const { t } = useTranslation("shell");

  if (!isOnline) {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-warning/10 text-status-warning"
        title={t("sync.offline")}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{t("sync.offline")}</span>
      </span>
    );
  }

  if (state === "syncing") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   text-text-secondary"
        title={t("sync.syncing")}
      >
        <RefreshCw className="h-3.5 w-3.5 animate-spin-slow" />
        <span className="hidden sm:inline">{t("sync.syncing")}</span>
      </span>
    );
  }

  if (state === "error") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-danger/10 text-status-danger"
        title={t("sync.error")}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{t("sync.error")}</span>
      </span>
    );
  }

  return null;
}
