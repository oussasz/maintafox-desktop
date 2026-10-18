/**
 * Database state for the status bar: the backend's health probe (`health_check`, a real
 * `SELECT 1` with a 2 s limit) asked once at start and then every minute while the app is open.
 * The bar shows exactly what the probe said; when the probe itself cannot be asked, the state is
 * "unknown" — never a green it does not know.
 */
import { useCallback, useEffect, useState } from "react";

import { healthCheck } from "@/services/app.service";
import type { HealthCheckResponse } from "@shared/ipc-types";

/** Pause between two probes while the app is open. */
export const DB_HEALTH_INTERVAL_MS = 60_000;

export type DbHealth =
  | { state: "checking" }
  | { state: "ok"; latencyMs: number | null }
  | { state: "degraded"; errorCode: HealthCheckResponse["db_error_code"] }
  | { state: "unknown" };

export function toDbHealth(response: HealthCheckResponse): DbHealth {
  if (response.status === "ok" && response.db_connected) {
    return { state: "ok", latencyMs: response.db_latency_ms ?? null };
  }
  return { state: "degraded", errorCode: response.db_error_code ?? null };
}

export function useDbHealth(intervalMs: number = DB_HEALTH_INTERVAL_MS): DbHealth {
  const [health, setHealth] = useState<DbHealth>({ state: "checking" });

  const probe = useCallback(async () => {
    try {
      setHealth(toDbHealth(await healthCheck()));
    } catch {
      setHealth({ state: "unknown" });
    }
  }, []);

  useEffect(() => {
    void probe();
    const timer = window.setInterval(() => void probe(), intervalMs);
    return () => window.clearInterval(timer);
  }, [probe, intervalMs]);

  return health;
}
