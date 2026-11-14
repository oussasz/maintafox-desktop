import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";

import { invoke } from "@/lib/ipc-invoke";
import { getAppInfo, healthCheck } from "@/services/app.service";
import { getUpdateJournal } from "@/services/updater-service";
import { useAppStore } from "@/store/app-store";
import type { StartupEvent } from "@shared/ipc-types";

/** Pauses between the fallback probes while start-up has not reported (the last one repeats). */
export const STARTUP_PROBE_DELAYS_MS = [400, 800, 1600, 3000];
/** After this long still "loading", the screen offers a retry instead of a spinner. */
export const STARTUP_STALL_AFTER_MS = 30_000;

/**
 * Subscribes to Tauri `startup_event` and transitions the app store
 * from "loading" → "ready", "error", "database_key_required" or
 * "update_rollback_required".
 *
 * The window may start listening after the backend said "ready", and the one fallback probe may
 * land while a long write holds the database. So the bridge asks again, with growing pauses, for
 * the backend's last start-up event (`get_last_startup_event`, no database needed) and a health
 * check, until start-up is no longer "loading". After `STARTUP_STALL_AFTER_MS` it flags the start
 * as stalled (the screen then offers a retry) and keeps probing: a late "ready" still gets in.
 *
 * In non-Tauri contexts (e.g. Vite dev server in browser), automatically
 * transitions to "ready" so the shell is usable for development.
 */
export function useStartupBridge(): void {
  const setAppStatus = useAppStore((s) => s.setAppStatus);
  const setAppVersion = useAppStore((s) => s.setAppVersion);
  const setStartupStalled = useAppStore((s) => s.setStartupStalled);
  const setUpdateRecovery = useAppStore((s) => s.setUpdateRecovery);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    let stallTimer: ReturnType<typeof setTimeout> | undefined;
    let probeTimer: ReturnType<typeof setTimeout> | undefined;

    const isLoading = () => useAppStore.getState().appStatus === "loading";

    function applyEvent(payload: StartupEvent) {
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
            backupRefusal: payload.backup_refusal ?? null,
          });
          break;
        default:
          break;
      }
    }

    /** One look at the backend: its last start-up event, then a health check. */
    async function probeBackend() {
      try {
        const last = await invoke<StartupEvent | null>("get_last_startup_event");
        if (!cancelled && last && typeof last.stage === "string" && isLoading()) {
          applyEvent(last);
        }
      } catch {
        // Older backend without the command: the health check below still covers `ready`.
      }
      if (cancelled || !isLoading()) return;
      try {
        const hc = await healthCheck();
        if (!cancelled && hc.db_connected && isLoading()) {
          setAppStatus("ready");
        }
      } catch {
        // Backend busy or not up yet (e.g. a long sync upload holds the database): ask again.
      }
    }

    async function probeUntilSettled() {
      let attempt = 0;
      while (!cancelled && isLoading()) {
        await probeBackend();
        if (cancelled || !isLoading()) return;
        const delay =
          STARTUP_PROBE_DELAYS_MS[Math.min(attempt, STARTUP_PROBE_DELAYS_MS.length - 1)] ?? 3000;
        attempt += 1;
        await new Promise<void>((resolve) => {
          probeTimer = setTimeout(resolve, delay);
        });
      }
    }

    async function bootstrap() {
      try {
        const unlistenFn = await listen<StartupEvent>("startup_event", (event) => {
          applyEvent(event.payload);
        });

        if (cancelled) {
          unlistenFn();
          return;
        }
        unlisten = unlistenFn;

        // From here the start must end: after a while the screen offers a retry.
        stallTimer = setTimeout(() => {
          if (!cancelled && isLoading()) setStartupStalled(true);
        }, STARTUP_STALL_AFTER_MS);

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
        // reinstalled. Checked before the fallback probes, which must not say "ready".
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
                backupRefusal: journal.backup_refusal,
              });
            }
          } catch {
            // Journal unreadable: the startup event (if any) still drives the status.
          }
        }

        // Guard against race condition: if the Rust backend emitted "ready" (or failed) before
        // this listener was registered, the event was missed. Ask the backend, again and again
        // with growing pauses, until start-up is settled. Only from "loading": never over an
        // error / recovery status.
        await probeUntilSettled();
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
      if (stallTimer !== undefined) clearTimeout(stallTimer);
      if (probeTimer !== undefined) clearTimeout(probeTimer);
      unlisten?.();
    };
  }, [setAppStatus, setAppVersion, setStartupStalled, setUpdateRecovery]);
}
