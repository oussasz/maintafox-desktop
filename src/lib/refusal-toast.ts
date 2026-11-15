import { useEffect } from "react";

import { describeAppError } from "@/lib/app-error";
import { useAppToastStore } from "@/store/app-toast-store";

/**
 * A refused right is said once: when a page or dialog shows the refusal itself, the global
 * toast the failed call raised for the same refusal is withdrawn. Used by the shared inline
 * error display (`ErrorState`) and by any banner that prints a refusal.
 */
export function withdrawRefusalToast(error: unknown): void {
  const { code, title, description } = describeAppError(error);
  if (code !== "PERMISSION_DENIED") return;
  useAppToastStore.getState().dismissMatching(title, description);
}

export function useWithdrawRefusalToast(error: unknown): void {
  useEffect(() => {
    if (error != null) withdrawRefusalToast(error);
  }, [error]);
}
