import { appErrorReason } from "@/lib/app-error";
import { isReportedPmError } from "@/services/pm-service";

/**
 * Text a PM dialog may show for a failed action: nothing when the global IPC toast already
 * reported it (no double reporting), the translated description otherwise (e.g. a response the
 * app could not read).
 */
export function pmUnreportedErrorText(err: unknown): string | null {
  if (isReportedPmError(err)) return null;
  return appErrorReason(err);
}
