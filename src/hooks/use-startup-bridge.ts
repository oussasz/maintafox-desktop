import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { getAppInfo, healthCheck } from "@/services/app.service";
import { getUpdateJournal } from "@/services/updater-service";
import { useAppStore } from "@/store/app-store";
import type { StartupEvent } from "@shared/ipc-types";

/**
 * Subscribes to Tauri `startup_event` and transitions the app store
 * from "loading" → "ready", "error", "database_key_required" or
 * "update_rollback_required".
 *
 * In non-Tauri contexts (e.g. Vite dev server in browser), automatically
 * transitions to "ready" so the shell is usable for development.
 */
export function useStartupBridge(): void {
  const setAppStatus = useAppStore((s) => s.setAppStatus);
  const setAppVersion = useAppStore((s) => s.setAppVersion);
  const setUpdateRecovery = useAppStore((s) => s.setUpdateRecovery);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    async function bootstrap() {
      try {
        const unlistenFn = await listen<StartupEvent>("startup_event", (event) => {
          const payload = event.payload;
          switch (payload.stage) {
            case "db_ready":
              setAppStatus("loading", "Base de données prête");
              break;
            case "migrations_complete":
              setAppStatus("loading", "Migrations appliquées");
              break;
            case "entitlement_cache_loaded":
              setAppStatus("loading", "Configuration chargée");
              break;
            case "ready":
              setAppStatus("ready");
              break;
            case "failed":
              setAppStatus("error", payload.reason || "Erreur de démarrage");
              break;
            case "database_key_required":
              setAppStatus("database_key_required", payload.reason);
              break;
            case "update_rollback_required":
              setUpdateRecovery({
                fromVersion: payload.from_version,
                toVersion: payload.to_version,
                reason: payload.reason,
              });
              break;
            default:
              break;
          }
        });

        if (cancelled) {
          unlistenFn();
          return;
        }
        unlisten = unlistenFn;

        // Fetch app version via IPC (also needed by the update recovery guard below).
        let runningVersion: string | null = null;
        try {
          const info = await getAppInfo();
          runningVersion = info.version;
          if (!cancelled) {
            setAppVersion(info.version);
          }
        } catch {
          // Non-critical; version display will show fallback
        }

        // Guard against race condition: "update_rollback_required" may have been emitted
        // before this listener was registered. The update journal says so durably: this
        // version (to_version) could not update the database and the previous one must be
        // reinstalled. Checked before the health_check fallback, which must not say "ready".
        if (runningVersion !== null) {
          try {
            const journal = await getUpdateJournal();
            if (
              !cancelled &&
              journal?.state === "rollback_required" &&
              journal.to_version === runningVersion
            ) {
              setUpdateRecovery({
                fromVersion: journal.from_version,
                toVersion: journal.to_version,
                reason: journal.failure ?? "",
              });
            }
          } catch {
            // Journal unreadable: the startup event (if any) still drives the status.
          }
        }

        // Guard against race condition: if the Rust backend emitted "ready"
        // before this listener was registered, the event was missed.
        // Call health_check as a fallback — if it succeeds, the backend is up.
        // Only from "loading": never over an error / recovery status.
        try {
          const hc = await healthCheck();
          if (!cancelled && hc.db_connected && useAppStore.getState().appStatus === "loading") {
            setAppStatus("ready");
          }
        } catch {
          // Backend not ready yet — the event listener will handle it
        }
      } catch {
        // Not running inside Tauri — set ready for browser dev mode
        if (!cancelled) {
          setAppStatus("ready");
          setAppVersion("0.1.0-dev");
        }
      }
    }

    void bootstrap();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [setAppStatus, setAppVersion, setUpdateRecovery]);
}
