import {
  AlertCircle,
  Clock,
  Cloud,
  LogOut,
  Menu,
  RefreshCw,
  Settings,
  Shield,
  ShieldAlert,
  User,
  UserCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router-dom";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { NotificationBell } from "@/components/notifications/NotificationBell";
import { OPEN_SYNC_CENTER_EVENT } from "@/components/notifications/notification-links";
import { CommandPalette } from "@/components/shell/CommandPalette";
import { SyncCenterModal } from "@/components/sync/SyncCenterModal";
import { useDeviceTrustStatus } from "@/hooks/use-device-trust-status";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { isBlockingGateActive } from "@/lib/blocking-gate";
import { useAppStore } from "@/store/app-store";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { DeviceTrustStatus } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function TopBar() {
  const { t } = useTranslation("shell");
  const toggleSidebar = useAppStore((s) => s.toggleSidebar);
  const syncStatus = useAppStore((s) => s.syncStatus);
  const syncErrorMessage = syncStatus.errorMessage;
  const isOnline = useAppStore((s) => s.isOnline);
  const displayName = useAppStore((s) => s.currentUserDisplayName);
  const { info: sessionInfo, logout: sessionLogout } = useSession();
  const deviceTrust = useDeviceTrustStatus();

  const navigate = useNavigate();
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [syncCenterOpen, setSyncCenterOpen] = useState(false);
  // The sync centre lists conflicts and history (`sync.view`): without it there is no button, and
  // the store does not refresh that list in the background either.
  const { can, isLoading: permissionsLoading } = usePermissions();
  // During a reload the last known rights stay in the context: the store keeps its answer meanwhile.
  const canViewSync = can(P.SYNC_VIEW);
  const setCanViewSync = useSyncOrchestratorStore((s) => s.setCanViewSync);
  useEffect(() => {
    if (!permissionsLoading) setCanViewSync(canViewSync);
  }, [canViewSync, permissionsLoading, setCanViewSync]);
  const syncConflicts = useSyncOrchestratorStore((s) => s.conflictInbox.length);
  const rejectedOutbox = useSyncOrchestratorStore((s) => s.rejectedOutboxCount);
  const syncNeedsAttention = syncConflicts > 0 || rejectedOutbox > 0;
  const userMenuRef = useRef<HTMLDivElement>(null);

  // A synchronisation-conflict notification opens the sync center (`notification-links`).
  useEffect(() => {
    if (!canViewSync) return;
    const open = () => setSyncCenterOpen(true);
    window.addEventListener(OPEN_SYNC_CENTER_EVENT, open);
    return () => window.removeEventListener(OPEN_SYNC_CENTER_EVENT, open);
  }, [canViewSync]);

  // Ctrl+K / ⌘K global shortcut for command palette
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      // Nothing opens under a blocking screen (forced lock removal acknowledgement).
      if (isBlockingGateActive()) return;
      if ((e.metaKey || e.ctrlKey) && e.key === "k") {
        e.preventDefault();
        setCommandPaletteOpen((v) => !v);
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, []);

  // Close dropdown on outside click
  useEffect(() => {
    function handleClickOutside(e: MouseEvent) {
      if (userMenuRef.current && !userMenuRef.current.contains(e.target as Node)) {
        setUserMenuOpen(false);
      }
    }
    if (userMenuOpen) {
      document.addEventListener("mousedown", handleClickOutside);
    }
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, [userMenuOpen]);

  const handleLogout = useCallback(async () => {
    setUserMenuOpen(false);
    try {
      // The session store also clears the permission cache and the session state.
      await sessionLogout();
    } finally {
      navigate("/login", { replace: true });
    }
  }, [navigate, sessionLogout]);

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

      {/* Logo + wordmark (desktop shell) */}
      <MaintafoxWordmark size="sm" className="mr-4 shrink-0" />

      {/* Search — opens command palette */}
      <div className="flex-1 hidden md:flex items-center">
        <button
          type="button"
          onClick={() => setCommandPaletteOpen(true)}
          aria-keyshortcuts="Control+K"
          className="flex h-9 w-72 items-center gap-2 rounded-md border border-surface-border
                     bg-surface-2 px-3 text-sm text-text-muted transition-colors duration-fast
                     hover:border-primary-light focus-visible:outline-none focus-visible:ring-2
                     focus-visible:ring-ring"
        >
          <span className="truncate">{t("commandPalette.open")}</span>
          <kbd className="ms-auto rounded border border-surface-border bg-surface-1 px-1.5 font-mono text-2xs">
            Ctrl K
          </kbd>
        </button>
      </div>
      <CommandPalette open={commandPaletteOpen} onOpenChange={setCommandPaletteOpen} />

      {/* Right controls */}
      <div className="ml-auto flex items-center gap-1">
        {/* Sync status indicator */}
        <SyncIndicator
          state={syncStatus.state}
          isOnline={isOnline}
          errorMessage={syncErrorMessage}
        />

        {canViewSync ? (
          <>
            <button
              type="button"
              onClick={() => setSyncCenterOpen(true)}
              title={t("sync.syncCenter")}
              className={`btn-ghost relative px-2 py-1.5 ${syncNeedsAttention ? "text-status-danger" : ""}`}
              aria-label={t("sync.syncCenter")}
            >
              <Cloud
                className={`h-4 w-4 ${syncNeedsAttention ? "animate-pulse drop-shadow-[0_0_6px_rgba(239,68,68,0.85)]" : ""}`}
              />
              {syncNeedsAttention ? (
                <span className="absolute right-1 top-1 h-2 w-2 rounded-full bg-status-danger ring-2 ring-surface-1" />
              ) : null}
            </button>
            <SyncCenterModal open={syncCenterOpen} onOpenChange={setSyncCenterOpen} />
          </>
        ) : null}

        {/* Notification bell */}
        <NotificationBell />

        {/* User menu */}
        <div ref={userMenuRef} className="relative">
          <button
            onClick={() => setUserMenuOpen((v) => !v)}
            aria-label={displayName ?? t("user.menu")}
            aria-expanded={userMenuOpen}
            aria-haspopup="true"
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

          {/* Dropdown */}
          {userMenuOpen && (
            <div
              className="absolute right-0 top-full mt-1 w-48 rounded-md border
                         border-surface-border bg-surface-1 py-1 shadow-lg z-50"
              role="menu"
            >
              {displayName && (
                <div
                  className="px-3 py-2 text-sm font-medium text-text-primary
                                border-b border-surface-border truncate"
                >
                  {displayName}
                </div>
              )}
              {/* Session & device trust info */}
              <div className="px-3 py-2 space-y-1 border-b border-surface-border">
                <SessionTimeIndicator expiresAt={sessionInfo?.expires_at ?? null} />
                <DeviceTrustBadge status={deviceTrust} />
              </div>
              <Link
                to="/settings?tab=profile"
                onClick={() => setUserMenuOpen(false)}
                className="flex items-center gap-2 px-3 py-2 text-sm text-text-secondary
                           hover:bg-surface-2 transition-colors"
                role="menuitem"
              >
                <UserCircle className="h-4 w-4" />
                {t("user.profile")}
              </Link>
              <Link
                to="/settings"
                onClick={() => setUserMenuOpen(false)}
                className="flex items-center gap-2 px-3 py-2 text-sm text-text-secondary
                           hover:bg-surface-2 transition-colors"
                role="menuitem"
              >
                <Settings className="h-4 w-4" />
                {t("user.settings")}
              </Link>
              <div className="border-t border-surface-border my-1" />
              <button
                onClick={handleLogout}
                className="flex w-full items-center gap-2 px-3 py-2 text-sm
                           text-status-danger hover:bg-surface-2 transition-colors"
                role="menuitem"
              >
                <LogOut className="h-4 w-4" />
                {t("user.logout")}
              </button>
            </div>
          )}
        </div>
      </div>
    </header>
  );
}

function SyncIndicator({
  state,
  isOnline,
  errorMessage,
}: {
  state: string;
  isOnline: boolean;
  errorMessage: string | null;
}) {
  const { t } = useTranslation("shell");

  const errorDetailTitle =
    state === "error" && errorMessage?.trim()
      ? `${t("sync.error")}: ${errorMessage}`
      : t("sync.error");

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

  if (state === "running") {
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

  if (state === "degraded") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-warning/10 text-status-warning"
        title={t("sync.indicator.degradedTitle")}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{t("sync.indicator.degraded")}</span>
      </span>
    );
  }

  if (state === "blocked" || state === "paused") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-warning/10 text-status-warning"
        title={
          state === "paused" ? t("sync.indicator.pausedTitle") : t("sync.indicator.blockedTitle")
        }
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">
          {state === "paused" ? t("sync.indicator.paused") : t("sync.indicator.blocked")}
        </span>
      </span>
    );
  }

  if (state === "error") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-danger/10 text-status-danger max-w-[min(28rem,55vw)]"
        title={errorDetailTitle}
      >
        <AlertCircle className="h-3.5 w-3.5 shrink-0" />
        <span className="hidden sm:inline truncate">{t("sync.error")}</span>
      </span>
    );
  }

  if (state === "scheduled") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs text-text-muted"
        title={t("sync.indicator.scheduled")}
      >
        <Clock className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{t("sync.indicator.scheduled")}</span>
      </span>
    );
  }

  return null;
}

/* ── Session time remaining ───────────────────────────────────────────── */

function SessionTimeIndicator({ expiresAt }: { expiresAt: string | null }) {
  const { t } = useTranslation("shell");
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!expiresAt) return;
    const id = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(id);
  }, [expiresAt]);

  const label = useMemo(() => {
    if (!expiresAt) return null;
    const diff = new Date(expiresAt).getTime() - now;
    if (diff <= 0) return t("session.expired");
    const mins = Math.ceil(diff / 60_000);
    return t("session.timeRemaining", { minutes: mins });
  }, [expiresAt, now, t]);

  if (!label) return null;

  return (
    <span className="flex items-center gap-1.5 text-xs text-text-muted">
      <Clock className="h-3 w-3" />
      {label}
    </span>
  );
}

/* ── Device trust badge ───────────────────────────────────────────────── */

function DeviceTrustBadge({ status }: { status: DeviceTrustStatus }) {
  const { t } = useTranslation("shell");

  // Same canonical label as License Enforcement (`trust_state` from trusted_devices).
  if (status.trust_state === "trusted") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-status-success">
        <Shield className="h-3 w-3" />
        {t("device.trusted")}
      </span>
    );
  }

  if (status.trust_state === "revoked") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-status-danger">
        <ShieldAlert className="h-3 w-3" />
        {t("device.untrusted")}
      </span>
    );
  }

  return (
    <span className="flex items-center gap-1.5 text-xs text-status-warning">
      <ShieldAlert className="h-3 w-3" />
      {t("device.untrusted")}
    </span>
  );
}
