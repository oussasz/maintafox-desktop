/**
 * Central IPC entry: all `invoke` calls go through here so failures can be surfaced consistently.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

import { i18n } from "@/i18n/config";
import { pushAppToast } from "@/store/app-toast-store";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { extractIpcValidationDetails, formatOrgIpcError, toErrorMessage } from "@/utils/errors";
import { formatOrgIssuesWithI18n } from "@/lib/format-org-validation-issue";

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
  // re-enter the password in `StepUpDialog` (via `useStepUp`).
  // `SESSION_LOCKED` is idle-lock: AuthGuard shows LockScreen after a silent refresh.

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

function isSessionConfirmCode(code: string): boolean {
  return (
    code === "SESSION_LOCKED" ||
    code === "AUTH_ERROR" ||
    code === "SESSION_CLAIM_INVALID" ||
    code === "TENANT_SCOPE_VIOLATION" ||
    code === "ACCOUNT_LOCKED"
  );
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

/**
 * Refresh session via silent IPC and push into the shared session store.
 * Dynamic import avoids a static cycle: session-store → auth-service → ipc-invoke.
 */
async function refreshSessionStoreSilent(): Promise<{
  is_authenticated: boolean;
  is_locked: boolean;
} | null> {
  try {
    const { getSessionInfo } = await import("@/services/auth-service");
    const { useSessionStore } = await import("@/store/session-store");
    const info = await getSessionInfo();
    useSessionStore.setState({
      info,
      isLoading: false,
      error: null,
      errorCode: null,
      hasBootstrapped: true,
    });
    return { is_authenticated: info.is_authenticated, is_locked: info.is_locked };
  } catch {
    return null;
  }
}

/**
 * Confirm session state before opening AuthLock. Idle-lock routes to LockScreen
 * via AuthGuard after refresh — never AuthLock. True session loss opens AuthLock.
 */
function handleAuthAndPermissionFailures(cmd: string, err: unknown): void {
  const { code, rawMsg } = extractIpcErrorMeta(err);

  if (SILENT_IPC_COMMANDS.has(cmd) || cmd === "login") {
    return;
  }

  if (isSessionConfirmCode(code)) {
    void (async () => {
      const snapshot = await refreshSessionStoreSilent();
      if (!snapshot) {
        // Could not refresh — only open AuthLock for true AUTH-style codes, not idle-lock.
        if (code === "SESSION_LOCKED") {
          return;
        }
        useAuthInterceptorStore.getState().openFromAuthFailure({
          mode: "session",
          failure: {
            atMs: Date.now(),
            command: cmd,
            code: code || null,
            message: rawMsg,
          },
        });
        return;
      }

      if (snapshot.is_locked) {
        // AuthGuard LockScreen owns idle-lock recovery.
        return;
      }

      if (snapshot.is_authenticated) {
        // Transient race: session is still valid.
        return;
      }

      if (code === "SESSION_LOCKED") {
        // Refresh showed unlocked+unauthenticated — treat as session loss.
        useAuthInterceptorStore.getState().openFromAuthFailure({
          mode: "session",
          failure: {
            atMs: Date.now(),
            command: cmd,
            code: code || null,
            message: rawMsg,
          },
        });
        return;
      }

      useAuthInterceptorStore.getState().openFromAuthFailure({
        mode: "session",
        failure: {
          atMs: Date.now(),
          command: cmd,
          code: code || null,
          message: rawMsg,
        },
      });
    })();
    return;
  }

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

  if (isSessionConfirmCode(code) || shouldOpenAuthLock(cmd, code)) {
    // AuthLock / LockScreen replace toast spam for auth/permission surfacing.
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
  const gateCodeMatch = rawMsg.match(
    /(GATE_[A-Z0-9_]+)(?::[A-Za-z0-9_=,\-./]+)?/,
  );
  const gateCode = gateCodeMatch?.[1] ?? "";
  const gateI18nKey = gateCode ? `errors:gateError.${gateCode}` : "";
  const title =
    gateI18nKey && i18n.exists(gateI18nKey)
      ? String(i18n.t(gateI18nKey))
      : i18nKey && i18n.exists(i18nKey)
        ? String(i18n.t(i18nKey))
        : rawMsg;

  const details = extractIpcValidationDetails(err);
  const coded = details.filter((d) => d.code);
  let desc: string | null = null;
  if (coded.length > 0) {
    desc = formatOrgIssuesWithI18n(coded);
  } else if (details.length > 0) {
    desc = details.map((d) => d.message).join("; ");
  } else if (title !== rawMsg && rawMsg) {
    desc = formatOrgIpcError(err);
  }

  // Avoid duplicating the same text in title and description.
  if (desc && desc === title) {
    desc = null;
  }

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
