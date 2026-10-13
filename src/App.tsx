import { useEffect } from "react";
import { RouterProvider } from "react-router-dom";

import { LocaleHtmlDir } from "@/components/locale-html-dir";
import { UpdateRecoveryScreen } from "@/components/startup/UpdateRecoveryScreen";
import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { useStartupBridge } from "@/hooks/use-startup-bridge";
import { router } from "@/router";
import { useAppStore } from "@/store/app-store";
import { useSettingsStore } from "@/stores/settings-store";

export function App() {
  // Bridge Tauri startup events → app store. Mounted here (not in the signed-in shell) so a
  // failed update is handled before sign-in: the recovery screen needs no session.
  useStartupBridge();
  const updateRecovery = useAppStore((s) =>
    s.appStatus === "update_rollback_required" ? s.updateRecovery : null,
  );

  useEffect(() => {
    void useSettingsStore.getState().loadSessionPolicy();
  }, []);

  return (
    <ThemeProvider>
      <LocaleHtmlDir />
      {updateRecovery ? (
        <UpdateRecoveryScreen
          fromVersion={updateRecovery.fromVersion}
          toVersion={updateRecovery.toVersion}
          reason={updateRecovery.reason}
        />
      ) : (
        <RouterProvider
          router={router}
          future={{
            v7_startTransition: true,
          }}
        />
      )}
    </ThemeProvider>
  );
}
