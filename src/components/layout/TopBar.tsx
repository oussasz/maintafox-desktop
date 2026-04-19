import {
  RefreshCw,
  AlertCircle,
  User,
  LogOut,
  Settings,
  UserCircle,
  Shield,
  ShieldAlert,
  Clock,
  Cloud,
} from "lucide-react";
import { useState, useRef, useEffect, useCallback, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, Link } from "react-router-dom";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { NotificationBell } from "@/components/notifications/NotificationBell";
import { CommandPalette } from "@/components/shell/CommandPalette";
import { SyncCenterModal } from "@/components/sync/SyncCenterModal";
import { useDeviceTrustStatus } from "@/hooks/use-device-trust-status";
import { useSession } from "@/hooks/use-session";
import { logout as authLogout } from "@/services/auth-service";
import { useAppStore } from "@/store/app-store";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";
import type { DeviceTrustStatus } from "@shared/ipc-types";

export function TopBar() {
  const { t } = useTranslation("shell");
  const syncStatus = useAppStore((s) => s.syncStatus);
  const syncErrorMessage = syncStatus.errorMessage;
  const isOnline = useAppStore((s) => s.isOnline);
  const displayName = useAppStore((s) => s.currentUserDisplayName);
  const { info: sessionInfo } = useSession();
  const deviceTrust = useDeviceTrustStatus();

  const navigate = useNavigate();
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [syncCenterOpen, setSyncCenterOpen] = useState(false);
  const syncConflicts = useSyncOrchestratorStore((s) => s.conflictInbox.length);
  const rejectedOutbox = useSyncOrchestratorStore((s) => s.rejectedOutboxCount);
  const syncNeedsAttention = syncConflicts > 0 || rejectedOutbox > 0;
  const userMenuRef = useRef<HTMLDivElement>(null);

  // Ctrl+K / ⌘K global shortcut for command palette
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
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
      await authLogout();
    } finally {
      navigate("/login", { replace: true });
    }
  }, [navigate]);

  return (
    <header
      className="fixed inset-x-0 top-0 z-30 flex h-topbar items-center
                 border-b border-surface-border bg-surface-1 px-3 gap-2"
      data-tauri-drag-region
    >
      {/* Logo + wordmark (desktop shell) */}
      <MaintafoxWordmark size="sm" className="mr-4 shrink-0" />

      {/* Search — opens command palette */}
      <div className="flex-1 hidden md:flex items-center">
        <div
          onClick={() => setCommandPaletteOpen(true)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") setCommandPaletteOpen(true);
          }}
          role="button"
          tabIndex={0}
          className="flex items-center gap-2 rounded-md border border-surface-border
                     bg-surface-2 px-3 py-1 text-sm text-text-muted cursor-pointer
                     hover:border-primary-light transition-colors duration-fast w-72"
        >
          <span>⌘K</span>
          <span>{t("search.placeholder")}</span>
        </div>
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
                to="/profile"
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
        title={t("sync.error")}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">Degraded</span>
      </span>
    );
  }

  if (state === "blocked" || state === "paused") {
    return (
      <span
        className="flex items-center gap-1 rounded px-2 py-1 text-xs
                   bg-status-warning/10 text-status-warning"
        title={state === "paused" ? "Sync paused by operator policy" : "Sync blocked by policy"}
      >
        <AlertCircle className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">{state === "paused" ? "Paused" : "Blocked"}</span>
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
        title="Sync scheduled"
      >
        <Clock className="h-3.5 w-3.5" />
        <span className="hidden sm:inline">Scheduled</span>
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

  if (status.status === "trusted") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-status-success">
        <Shield className="h-3 w-3" />
        {t("device.trusted")}
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
