import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";

import { healthCheck } from "@/services/app.service";
import { toErrorMessage } from "@/utils/errors";
import type { StartupEvent } from "@shared/ipc-types";

export type MobileStartupState =
  | {
      status: "loading";
      stage: "waiting" | "db_ready" | "migrations_complete" | "entitlement_cache_loaded";
      applied: number | null;
    }
  | { status: "ready" }
  | { status: "failed"; reason: string }
  | { status: "database_key_required"; reason: string }
  | { status: "update_rollback_required"; from: string; to: string; reason: string };

const INITIAL: MobileStartupState = { status: "loading", stage: "waiting", applied: null };

/** The next state after a `startup_event`; terminal states are never left by a later progress event. */
export function reduceStartup(state: MobileStartupState, event: StartupEvent): MobileStartupState {
  switch (event.stage) {
    case "db_ready":
    case "entitlement_cache_loaded":
      return state.status === "loading"
        ? { status: "loading", stage: event.stage, applied: state.applied }
        : state;
    case "migrations_complete":
      return state.status === "loading"
        ? { status: "loading", stage: "migrations_complete", applied: event.applied }
        : state;
    case "ready":
      return { status: "ready" };
    case "failed":
      return { status: "failed", reason: event.reason };
    case "database_key_required":
      return { status: "database_key_required", reason: event.reason };
    case "update_rollback_required":
      return {
        status: "update_rollback_required",
        from: event.from_version,
        to: event.to_version,
        reason: event.backup_refusal ?? event.reason,
      };
    default:
      return state;
  }
}

/**
 * Follows the Tauri `startup_event`. Like the desktop bridge (`use-startup-bridge.ts`), it also
 * asks the engine for its last startup event (`get_last_startup_event`) and `health_check` once the
 * listener is attached, for an event emitted before it: the phone's page loads after the engine
 * started, so a failure (e.g. no screen lock) would otherwise never be shown. Outside the engine (no Tauri) the failure is
 * shown, never hidden behind a fake ready.
 */
export function useMobileStartup(): { state: MobileStartupState; retryProbe: () => void } {
  const [state, setState] = useState<MobileStartupState>(INITIAL);
  const stateRef = useRef(state);
  stateRef.current = state;
  const [probe, setProbe] = useState(0);

  const retryProbe = useCallback(() => {
    setState(INITIAL);
    setProbe((n) => n + 1);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    async function attach() {
      try {
        const unlistenFn = await listen<StartupEvent>("startup_event", (event) => {
          setState((current) => reduceStartup(current, event.payload));
        });
        if (cancelled) {
          unlistenFn();
          return;
        }
        unlisten = unlistenFn;
        // The phone's page loads after the engine started: apply the event it may have missed.
        try {
          const last = await invoke<StartupEvent | null>("get_last_startup_event");
          if (!cancelled && last) setState((current) => reduceStartup(current, last));
        } catch {
          // Older engine without the command: the health check below still covers `ready`.
        }
        try {
          const hc = await healthCheck();
          if (!cancelled && hc.db_connected && stateRef.current.status === "loading") {
            setState({ status: "ready" });
          }
        } catch {
          // The engine is not up yet: the event listener will report it.
        }
      } catch (err) {
        if (!cancelled) setState({ status: "failed", reason: toErrorMessage(err) });
      }
    }

    void attach();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [probe]);

  return { state, retryProbe };
}
