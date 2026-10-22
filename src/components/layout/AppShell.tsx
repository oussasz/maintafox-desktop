import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { AdminRecoveryNotice } from "@/components/auth/AdminRecoveryNotice";
import { BuiltinAdminWarning } from "@/components/auth/BuiltinAdminWarning";
import { SingleAdministratorWarning } from "@/components/auth/SingleAdministratorWarning";
import { SafetyRuleNotice } from "@/components/notifications/SafetyRuleNotice";
import { ForcedRemovalAcknowledgementGate } from "@/components/permits/ForcedRemovalAcknowledgementGate";
import { AppToastHost } from "@/components/shell/AppToastHost";
import { DatabaseRecoveryScreen } from "@/components/startup/DatabaseRecoveryScreen";
import { UpdateBanner } from "@/components/updates/UpdateBanner";
import { mfAuth } from "@/design-system/tokens";
import { useUpdateScheduler } from "@/hooks/use-updater";
import { invokeSilent } from "@/lib/ipc-invoke";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import { cn } from "@/lib/utils";
import { defaultNavItems } from "@/navigation/nav-registry";
import { useAppStore } from "@/store/app-store";
import { useSessionStore } from "@/store/session-store";
import { useLocaleStore } from "@/stores/locale-store";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";

import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";

/** Keep idle-lock from firing during passive UI time (backend idle = 30 min). */
const SESSION_TOUCH_INTERVAL_MS = 5 * 60 * 1000;

interface AppShellProps {
  children: ReactNode;
}

export function AppShell({ children }: AppShellProps) {
  const { t } = useTranslation("shell");
  const appStatus = useAppStore((s) => s.appStatus);
  const startupMsg = useAppStore((s) => s.startupMessage);
  const collapsed = useAppStore((s) => s.sidebarCollapsed);
  const initializeSync = useSyncOrchestratorStore((s) => s.initialize);
  const shutdownSync = useSyncOrchestratorStore((s) => s.shutdown);
  const refreshCurrency = useLocaleStore((s) => s.refreshCurrency);
  const sessionAuthenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const sessionLocked = useSessionStore((s) => s.info?.is_locked === true);
  const sessionUserId = useSessionStore((s) => s.info?.user_id ?? null);
  // While the forced-removal acknowledgement screen is shown, the rest of the app is inert
  // (unreachable by mouse, keyboard and screen readers).
  const [gateActive, setGateActive] = useState(false);
  const shellRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const shell = shellRef.current;
    if (!shell) return;
    if (gateActive) {
      shell.setAttribute("inert", "");
      shell.setAttribute("aria-hidden", "true");
    } else {
      shell.removeAttribute("inert");
      shell.removeAttribute("aria-hidden");
    }
  }, [gateActive]);

  // Startup events are bridged to the app store in App (before sign-in). In-app update
  // checks run here once startup is complete: once now, then periodically.
  useUpdateScheduler(appStatus === "ready");

  useEffect(() => {
    initializeSync();
    return () => shutdownSync();
  }, [initializeSync, shutdownSync]);

  useEffect(() => {
    if (!sessionAuthenticated || sessionLocked) return;
    void refreshCurrency();
  }, [sessionAuthenticated, sessionLocked, refreshCurrency]);

  // Shell-level activity heartbeat — covers idle UI with no authenticated IPC.
  useEffect(() => {
    if (!sessionAuthenticated || sessionLocked) {
      return;
    }

    const touch = () => {
      if (!isSessionActiveForBackgroundWork()) {
        return;
      }
      void invokeSilent("touch_session").catch(() => {});
    };

    touch();
    const id = window.setInterval(touch, SESSION_TOUCH_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [sessionAuthenticated, sessionLocked]);

  if (appStatus === "loading") {
    return (
      <div
        className="flex h-screen flex-col items-center justify-center
                   bg-surface-0 gap-4"
      >
        <div
          className="h-8 w-8 animate-spin rounded-full border-2
                     border-surface-3 border-t-primary"
        />
        <p className="text-sm text-text-secondary">{startupMsg || t("startup.loading")}</p>
      </div>
    );
  }

  if (appStatus === "database_key_required") {
    return <DatabaseRecoveryScreen reason={startupMsg ?? ""} />;
  }

  if (appStatus === "error") {
    return (
      <div className={mfAuth.startupShell}>
        <p className="text-lg font-semibold text-text-danger">{t("startup.errorTitle")}</p>
        <p className="text-sm text-text-secondary max-w-md">{startupMsg}</p>
        <button className="btn-primary mt-2" onClick={() => window.location.reload()}>
          {t("startup.retry")}
        </button>
      </div>
    );
  }

  return (
    <>
      <div ref={shellRef} className="flex h-screen flex-col bg-surface-0" data-testid="app-shell">
        <TopBar />
        <div className={cn("flex flex-1 overflow-hidden", "pt-topbar pb-statusbar")}>
          <Sidebar items={defaultNavItems} />
          <main
            className={cn(
              "flex-1 overflow-auto transition-all duration-normal",
              collapsed ? "ml-sidebar-sm" : "ml-sidebar",
            )}
          >
            <UpdateBanner />
            <BuiltinAdminWarning sessionUserId={sessionUserId} className="m-4" />
            <SingleAdministratorWarning sessionUserId={sessionUserId} className="m-4" />
            <AdminRecoveryNotice sessionUserId={sessionUserId} className="m-4" />
            <SafetyRuleNotice sessionUserId={sessionUserId} className="m-4" />
            {children}
          </main>
        </div>
        <StatusBar />
        <AppToastHost />
      </div>
      {/* A lock of the signed-in worker taken off by a forced removal: acknowledged before anything else. */}
      {sessionAuthenticated && !sessionLocked ? (
        <ForcedRemovalAcknowledgementGate
          sessionUserId={sessionUserId}
          onActiveChange={setGateActive}
        />
      ) : null}
    </>
  );
}
