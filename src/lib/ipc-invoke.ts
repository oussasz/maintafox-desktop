/**
 * Central IPC entry: all `invoke` calls go through here so failures can be surfaced consistently.
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";

import { i18n } from "@/i18n/config";
import { ACCOUNT_LOCKED, lockoutUntil } from "@/lib/account-lockout";
import { describeAppError } from "@/lib/app-error";
import {
  hasStepUpRequester,
  isLocalStepUpActive,
  requestGlobalStepUp,
  STEP_UP_CANCELLED_MESSAGE,
} from "@/lib/step-up-bridge";
import { pushAppToast } from "@/store/app-toast-store";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import { toErrorMessage } from "@/utils/errors";

/** Commands invoked during bootstrap where a toast would be noisy or misleading. */
const SILENT_IPC_COMMANDS = new Set([
  "health_check",
  "get_app_info",
  "get_task_status",
  "set_window_mode",
]);

/**
 * The session ended with the account lockout (`ACCOUNT_LOCKED`; outside sign-in it only comes
 * from a password check inside the session): sign-in shows the lockout with its end.
 */
async function applyAccountLockout(err: unknown): Promise<void> {
  const { useSessionStore } = await import("@/store/session-store");
  useSessionStore.getState().applyAccountLockout(lockoutUntil(err));
}

/** Server code (`AppError::PasswordChangeRequired`): the password must be changed first. */
export const PASSWORD_CHANGE_REQUIRED = "PASSWORD_CHANGE_REQUIRED";

function extractIpcErrorMeta(err: unknown): { code: string; rawMsg: string } {
  const code =
    typeof err === "object" && err !== null && "code" in err
      ? String((err as { code: unknown }).code)
      : "";
  const rawMsg = toErrorMessage(err);
  return { code, rawMsg };
}

/**
 * Codes that mean the session is lost (sign in again), aligned with `AppError` serialization in
 * `src-tauri/src/errors.rs`. `STEP_UP_REQUIRED` is not one (the user stays signed in and confirms
 * the password in `StepUpDialog`); `SESSION_LOCKED` is the idle lock (AuthGuard's LockScreen).
 * `PERMISSION_DENIED` is not one either: a refused right never opens the lock, it is one toast
 * with the reason and the app stays usable (AU-35).
 */
function isLostSessionCode(code: string): boolean {
  return (
    code === "AUTH_ERROR" ||
    code === "SESSION_CLAIM_INVALID" ||
    code === "TENANT_SCOPE_VIOLATION" ||
    code === "ACCOUNT_LOCKED"
  );
}

function isSessionConfirmCode(code: string): boolean {
  return code === "SESSION_LOCKED" || isLostSessionCode(code);
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
    useSessionStore.getState().applySessionInfo(info);
    return { is_authenticated: info.is_authenticated, is_locked: info.is_locked };
  } catch {
    return null;
  }
}

/** The error says the session is lost (sign in again), not a permission or data problem. */
export function isLostSessionError(err: unknown): boolean {
  return isLostSessionCode(extractIpcErrorMeta(err).code);
}

/** Opens the sign-in-again dialog for a command refused because the session is lost. */
export function openSignInAgain(cmd: string, err: unknown): void {
  const { code, rawMsg } = extractIpcErrorMeta(err);
  useAuthInterceptorStore.getState().openFromAuthFailure({
    mode: "session",
    failure: { atMs: Date.now(), command: cmd, code: code || null, message: rawMsg },
  });
}

/** A command was refused as unauthenticated while the session is valid: screens reload once. */
async function noteAuthRace(): Promise<void> {
  const { useSessionStore } = await import("@/store/session-store");
  useSessionStore.getState().noteAuthRace();
}

/**
 * For calls made with `invokeSilent` (no lock, no toast) whose failure leaves a screen empty, such
 * as the permission list: on a session error, re-read the session; when it is in fact valid (an
 * authentication race), raise the session generation once so the screens load again. A session
 * really lost shows as such through the refreshed session.
 */
export async function confirmSessionAfterSilentFailure(err: unknown): Promise<void> {
  const { code } = extractIpcErrorMeta(err);
  if (!isSessionConfirmCode(code)) {
    return;
  }
  const snapshot = await refreshSessionStoreSilent();
  if (snapshot?.is_authenticated && !snapshot.is_locked) {
    await noteAuthRace();
  }
}

/**
 * Confirm session state before opening AuthLock. Idle-lock routes to LockScreen
 * via AuthGuard after refresh — never AuthLock. True session loss opens AuthLock.
 * Anything else (a refused right included) is left to the caller and the toast.
 */
function handleSessionFailures(cmd: string, err: unknown): void {
  const { code, rawMsg } = extractIpcErrorMeta(err);

  if (SILENT_IPC_COMMANDS.has(cmd) || cmd === "login") {
    return;
  }

  if (code === ACCOUNT_LOCKED) {
    // A password check inside the session (unlock, step-up, PIN set/clear, password change)
    // locked the account and the server ended the session: back to sign-in with the lockout
    // message (a refresh would wipe that message, the sign-in-again dialog would hide it).
    void applyAccountLockout(err);
    return;
  }

  if (code === PASSWORD_CHANGE_REQUIRED) {
    // The server refuses every command until the password is changed and flags the session:
    // the refreshed session routes AuthGuard to the change-password screen (no lock, no toast).
    void refreshSessionStoreSilent();
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
        // Transient race: the session is still valid. Screens that failed load again (once).
        await noteAuthRace();
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
  }
}

function notifyInvokeFailure(cmd: string, err: unknown): void {
  if (SILENT_IPC_COMMANDS.has(cmd)) {
    return;
  }
  const { code, rawMsg } = extractIpcErrorMeta(err);

  if (isSessionConfirmCode(code)) {
    // AuthLock / LockScreen replace toast spam for a lost or locked session.
    return;
  }

  if (code === "STEP_UP_REQUIRED") {
    // `useStepUp` shows `StepUpDialog` with a password field; no global lock/toast.
    return;
  }

  if (code === PASSWORD_CHANGE_REQUIRED) {
    // AuthGuard shows the change-password screen instead.
    return;
  }

  const isMissingCommand = /Command\s+.+\s+not found/i.test(rawMsg);
  if (isMissingCommand) {
    // The raw text names an internal command: console only, never in the toast.
    console.warn("[invoke] command not available", cmd, rawMsg);
    pushAppToast({
      title: i18n.t("errors:unexpectedError"),
      variant: "destructive",
    });
    return;
  }

  const { title, description: desc } = describeAppError(err);

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

export interface InvokeOptions {
  /**
   * Show the global error toast on failure (default true). Set false when the caller shows
   * the failure itself (inline, in plain language) or for calls the user did not ask for: a
   * refused right then only reaches the caller. Session-loss handling (the lock) still runs.
   */
  notifyOnError?: boolean;
}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  options?: InvokeOptions,
  afterStepUp = false,
): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (e) {
    // A caller without its own `withStepUp` still gets the password prompt, then a retry.
    if (
      extractIpcErrorMeta(e).code === "STEP_UP_REQUIRED" &&
      !afterStepUp &&
      !isLocalStepUpActive() &&
      hasStepUpRequester()
    ) {
      if (!(await requestGlobalStepUp())) {
        throw new Error(STEP_UP_CANCELLED_MESSAGE);
      }
      // One retry only: a second refusal after a verified password is shown as a refusal.
      return await invoke<T>(cmd, args, options, true);
    }
    handleSessionFailures(cmd, e);
    if (options?.notifyOnError !== false) {
      notifyInvokeFailure(cmd, e);
    }
    throw e;
  }
}
