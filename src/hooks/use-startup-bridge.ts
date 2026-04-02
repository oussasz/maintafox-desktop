import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { getAppInfo } from "@/services/app.service";
import { useAppStore } from "@/store/app-store";
import type { StartupEvent } from "@shared/ipc-types";

/**
 * Subscribes to Tauri `startup_event` and transitions the app store
 * from "loading" → "ready" or "error".
 *
 * In non-Tauri contexts (e.g. Vite dev server in browser), automatically
 * transitions to "ready" so the shell is usable for development.
 */
export function useStartupBridge(): void {
  const setAppStatus = useAppStore((s) => s.setAppStatus);
  const setAppVersion = useAppStore((s) => s.setAppVersion);

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
              setAppStatus("error", payload.reason ?? "Erreur de démarrage");
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

        // Fetch app version via IPC
        try {
          const info = await getAppInfo();
          if (!cancelled) {
            setAppVersion(info.version);
          }
        } catch {
          // Non-critical; version display will show fallback
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
  }, [setAppStatus, setAppVersion]);
}
