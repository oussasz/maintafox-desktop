/**
 * Central IPC entry: all `invoke` calls go through here so failures can be surfaced consistently.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

import { i18n } from "@/i18n/config";
import { pushAppToast } from "@/store/app-toast-store";
import { toErrorMessage } from "@/utils/errors";

/** Commands invoked during bootstrap where a toast would be noisy or misleading. */
const SILENT_IPC_COMMANDS = new Set(["health_check", "get_app_info", "get_task_status"]);

function notifyInvokeFailure(cmd: string, err: unknown): void {
  if (SILENT_IPC_COMMANDS.has(cmd)) {
    return;
  }
  const code =
    typeof err === "object" && err !== null && "code" in err
      ? String((err as { code: unknown }).code)
      : "";
  const rawMsg = toErrorMessage(err);
  const i18nKey = code ? `errors:appError.${code}` : "";
  const title = i18nKey && i18n.exists(i18nKey) ? String(i18n.t(i18nKey)) : rawMsg;
  const desc = title !== rawMsg && rawMsg ? rawMsg : null;
  pushAppToast({
    title,
    ...(desc ? { description: desc } : {}),
    variant: "destructive",
  });
}

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    notifyInvokeFailure(cmd, e);
    throw e;
  }
}
