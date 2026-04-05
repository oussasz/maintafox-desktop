import { invoke } from "@tauri-apps/api/core";
import { useState, useEffect, useCallback } from "react";

import type { DeviceTrustStatus } from "@shared/ipc-types";

/**
 * Fetches the current device's trust status on mount.
 *
 * If the IPC command fails (e.g., no active session), defaults to "unknown".
 * Re-fetches automatically when the component remounts.
 */
export function useDeviceTrustStatus(): DeviceTrustStatus {
  const [status, setStatus] = useState<DeviceTrustStatus>({ status: "unknown" });

  const fetch = useCallback(async () => {
    try {
      const result = await invoke<DeviceTrustStatus>("get_device_trust_status");
      setStatus(result);
    } catch {
      // Command failed (no session, or not implemented yet) — silent fallback
      setStatus({ status: "unknown" });
    }
  }, []);

  useEffect(() => {
    void fetch();
  }, [fetch]);

  return status;
}
