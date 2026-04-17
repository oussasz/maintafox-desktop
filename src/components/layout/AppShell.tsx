import { useEffect, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { useStartupBridge } from "@/hooks/use-startup-bridge";
import { cn } from "@/lib/utils";
import { defaultNavItems } from "@/navigation/nav-registry";
import { useAppStore } from "@/store/app-store";
import { useSyncOrchestratorStore } from "@/stores/sync-orchestrator-store";

import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { TopBar } from "./TopBar";

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

  // Bridge Tauri startup events → app store
  useStartupBridge();

  useEffect(() => {
    initializeSync();
    return () => shutdownSync();
  }, [initializeSync, shutdownSync]);

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
    </div>
  );
}
