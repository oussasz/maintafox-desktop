import { useEffect, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { AppToastHost } from "@/components/shell/AppToastHost";
import { useStartupBridge } from "@/hooks/use-startup-bridge";
import { invokeSilent } from "@/lib/ipc-invoke";
import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import { cn } from "@/lib/utils";
import { defaultNavItems } from "@/navigation/nav-registry";
import { useAppStore } from "@/store/app-store";
import { useSessionStore } from "@/store/session-store";
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
  const sessionAuthenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const sessionLocked = useSessionStore((s) => s.info?.is_locked === true);

  // Bridge Tauri startup events → app store
  useStartupBridge();

  useEffect(() => {
    initializeSync();
    return () => shutdownSync();
  }, [initializeSync, shutdownSync]);

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

  if (appStatus === "error") {
    return (
      <div
        className="flex h-screen flex-col items-center justify-center
                   bg-surface-0 gap-4 px-8 text-center"
      >
        <p className="text-lg font-semibold text-text-danger">{t("startup.errorTitle")}</p>
        <p className="text-sm text-text-secondary max-w-md">{startupMsg}</p>
        <button className="btn-primary mt-2" onClick={() => window.location.reload()}>
          {t("startup.retry")}
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-screen flex-col bg-surface-0">
      <TopBar />
      <div className={cn("flex flex-1 overflow-hidden", "pt-topbar pb-statusbar")}>
        <Sidebar items={defaultNavItems} />
        <main
          className={cn(
            "flex-1 overflow-auto transition-all duration-normal",
            collapsed ? "ml-sidebar-sm" : "ml-sidebar",
          )}
        >
          {children}
        </main>
      </div>
      <StatusBar />
      <AppToastHost />
    </div>
  );
}
