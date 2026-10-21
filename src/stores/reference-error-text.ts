/**
 * One translated line for a Reference data failure (IPC reject object, Error, string).
 *
 * Wraps `describeAppError` so the stores and panels of the module never show `[object Object]`,
 * raw JSON or an untranslated error code.
 */

import { describeAppError } from "@/lib/app-error";

export function referenceErrorText(err: unknown): string {
  const { title, description } = describeAppError(err);
  return description ? `${title} — ${description}` : title;
}
