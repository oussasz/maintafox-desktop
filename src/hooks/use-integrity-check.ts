import { useCallback, useState } from "react";

import { runIntegrityCheck, repairSeedData } from "@/services/diagnostics-service";
import type { IntegrityReport } from "@shared/ipc-types";

export type IntegrityStatus = "idle" | "checking" | "repairing" | "done" | "error";

export interface UseIntegrityCheckReturn {
  report: IntegrityReport | null;
  status: IntegrityStatus;
  error: string | null;
  check: () => Promise<void>;
  repair: () => Promise<void>;
}

export function useIntegrityCheck(): UseIntegrityCheckReturn {
  const [report, setReport] = useState<IntegrityReport | null>(null);
  const [status, setStatus] = useState<IntegrityStatus>("idle");
  const [error, setError] = useState<string | null>(null);

  const check = useCallback(async () => {
    setStatus("checking");
    setError(null);
    try {
      const result = await runIntegrityCheck();
      setReport(result);
      setStatus("done");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus("error");
    }
  }, []);

  const repair = useCallback(async () => {
    setStatus("repairing");
    setError(null);
    try {
      const result = await repairSeedData();
      setReport(result);
      setStatus("done");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStatus("error");
    }
  }, []);

  return { report, status, error, check, repair };
}
