/**
 * Central IPC entry: all `invoke` calls go through here so failures can be surfaced consistently.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

import { i18n } from "@/i18n/config";
import { pushAppToast } from "@/store/app-toast-store";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { toErrorMessage } from "@/utils/errors";

/** Commands invoked during bootstrap where a toast would be noisy or misleading. */
const SILENT_IPC_COMMANDS = new Set(["health_check", "get_app_info", "get_task_status"]);

type AuthLockMode = "session" | "permission" | "unknown";

function extractIpcErrorMeta(err: unknown): { code: string; rawMsg: string } {
  const code =
    typeof err === "object" && err !== null && "code" in err
      ? String((err as { code: unknown }).code)
      : "";
  const rawMsg = toErrorMessage(err);
  return { code, rawMsg };
}

function classifyAuthLock(code: string): AuthLockMode | null {
  // These align with `AppError` serialization in `src-tauri/src/errors.rs`.
  if (!code) return null;

  // `STEP_UP_REQUIRED` is *not* a lost session: the user remains logged in and must
  // re-enter the password in `StepUpDialog` (via `useStepUp`). Treating it like
  // "session" here opened `AuthLockLayer` without a password field (that UI only
  // shows the password when `!is_authenticated`).

  if (
    code === "AUTH_ERROR" ||
    code === "SESSION_CLAIM_INVALID" ||
    code === "TENANT_SCOPE_VIOLATION" ||
    code === "ACCOUNT_LOCKED"
  ) {
    return "session";
  }

  if (code === "PERMISSION_DENIED") {
    return "permission";
  }

  return null;
}

function shouldOpenAuthLock(cmd: string, code: string): boolean {
  if (SILENT_IPC_COMMANDS.has(cmd)) {
    return false;
  }
  // Login failures should remain local to the login form.
  if (cmd === "login") {
    return false;
  }
  return classifyAuthLock(code) !== null;
}

function handleAuthAndPermissionFailures(cmd: string, err: unknown): void {
  const { code, rawMsg } = extractIpcErrorMeta(err);
  if (!shouldOpenAuthLock(cmd, code)) {
    return;
  }

  const mode = classifyAuthLock(code) ?? "unknown";
  useAuthInterceptorStore.getState().openFromAuthFailure({
    mode,
    failure: {
      atMs: Date.now(),
      command: cmd,
      code: code || null,
      message: rawMsg,
    },
  });
}

function notifyInvokeFailure(cmd: string, err: unknown): void {
  if (SILENT_IPC_COMMANDS.has(cmd)) {
    return;
  }
  const { code, rawMsg } = extractIpcErrorMeta(err);

  if (shouldOpenAuthLock(cmd, code)) {
    // Central lock screen replaces toast spam for auth/permission surfacing.
    return;
  }

  if (code === "STEP_UP_REQUIRED") {
    // `useStepUp` shows `StepUpDialog` with a password field; no global lock/toast.
    return;
  }

  const isMissingCommand = /Command\s+.+\s+not found/i.test(rawMsg);
  if (isMissingCommand) {
    pushAppToast({
      title: i18n.t("errors:unexpectedError", {
        defaultValue:
          "This feature is temporarily unavailable. Please restart the app or update to the latest version.",
      }),
      description: rawMsg,
      variant: "destructive",
    });
    return;
  }

  const i18nKey = code ? `errors:appError.${code}` : "";
  const title = i18nKey && i18n.exists(i18nKey) ? String(i18n.t(i18nKey)) : rawMsg;
  const desc = title !== rawMsg && rawMsg ? rawMsg : null;
  pushAppToast({
    title,
    ...(desc ? { description: desc } : {}),
    variant: "destructive",
  });
}

/**
 * Raw invoke without global toast side effects. Prefer for bootstrap/session polling.
 */
export async function invokeSilent<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return await tauriInvoke<T>(cmd, args);
}

export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    handleAuthAndPermissionFailures(cmd, e);
    notifyInvokeFailure(cmd, e);
    throw e;
  }
}
