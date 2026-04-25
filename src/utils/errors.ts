/**
 * Extract a human-readable message from an unknown error value.
 *
 * Tauri's invoke() rejects with plain objects like `{ code, message }`,
 * not Error instances.  `String(obj)` yields "[object Object]" which is
 * useless in the UI.  This helper covers the common shapes:
 *
 *  - Error instances → err.message
 *  - Objects with a `message` string → obj.message
 *  - Strings → as-is
 *  - Everything else → JSON.stringify fallback
 */
export function toErrorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;
  if (typeof err === "object" && err !== null && "message" in err) {
    const msg = (err as { message: unknown }).message;
    if (typeof msg === "string") return msg;
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}
