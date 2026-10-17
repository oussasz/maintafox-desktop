/**
 * Inline text for a failed WO action.
 *
 * IPC rejects (objects with a `code`) and version conflicts are already reported by the
 * global IPC toast with the translated reason, so the inline line only repeats the
 * context ("Could not start the work order"). Any other failure (client-side parse or
 * runtime error) raises no toast, so its translated reason is appended here.
 */
import { describeAppError } from "@/lib/app-error";
import { extractIpcErrorCode } from "@/utils/errors";

function isToastedFailure(err: unknown): boolean {
  if (extractIpcErrorCode(err) != null) return true;
  return err instanceof Error && err.name === "VersionConflictError";
}

export function woActionFailureText(context: string, err: unknown): string {
  if (isToastedFailure(err)) return context;
  const { title } = describeAppError(err);
  return title && title !== context ? `${context} — ${title}` : context;
}
