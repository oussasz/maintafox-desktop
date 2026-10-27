import { create } from "zustand";

import { ACCOUNT_LOCKED, accountLockedText, lockoutUntil } from "@/lib/account-lockout";
import { clearPermissionCache } from "@/lib/permission-cache";
import {
  forceChangePassword as authForceChange,
  login as authLogin,
  logout as authLogout,
  unlockSession as authUnlock,
  getSessionInfo,
} from "@/services/auth-service";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import type { LoginRequest, SessionInfo } from "@shared/ipc-types";

/** Extract message and code from a Tauri IPC error object or standard Error. */
function extractTauriError(e: unknown, fallback: string): { message: string; code: string | null } {
  if (e instanceof Error) {
    return { message: e.message, code: null };
  }
  if (typeof e === "object" && e !== null && "message" in e && "code" in e) {
    const obj = e as { code: string; message: string };
    return { message: obj.message, code: obj.code };
  }
  return { message: fallback, code: null };
}

export { ACCOUNT_LOCKED } from "@/lib/account-lockout";

/**
 * The session failure to show: the account lockout in the app language with its local end
 * time (built at display time, so it follows a language switch), else the stored text.
 */
export function sessionErrorText(state: {
  error: string | null;
  errorCode: string | null;
  lockedUntil: string | null;
}): string | null {
  return state.errorCode === ACCOUNT_LOCKED ? accountLockedText(state.lockedUntil) : state.error;
}

export const UNAUTHENTICATED_SESSION: SessionInfo = {
  is_authenticated: false,
  is_locked: false,
  user_id: null,
  username: null,
  display_name: null,
  is_admin: null,
  force_password_change: null,
  expires_at: null,
  last_activity_at: null,
  idle_timeout_minutes: null,
  password_expires_in_days: null,
  pin_configured: null,
  tenant_id: null,
  token_tenant_id: null,
};

/** A usable session: signed in and not idle-locked. */
function isActive(info: SessionInfo | null): boolean {
  return info?.is_authenticated === true && !info.is_locked;
}

/**
 * The session generation after `next` replaces `prev`: raised when a session becomes usable
 * (sign-in, unlock, PIN unlock, re-login after a lost session), so screens that failed while
 * there was none (permissions, dashboard) load again.
 */
function generationAfter(prev: SessionInfo | null, next: SessionInfo, generation: number): number {
  return isActive(next) && !isActive(prev) ? generation + 1 : generation;
}

function applyAuthenticatedSideEffects(info: SessionInfo): void {
  if (!info.is_authenticated) {
    return;
  }
  useAuthInterceptorStore.getState().rememberPreInterrupt(info);
  if (useAuthInterceptorStore.getState().isLockOpen) {
    useAuthInterceptorStore.getState().clear();
  }
}

export interface SessionStoreState {
  info: SessionInfo | null;
  isLoading: boolean;
  /**
   * The failure's text. Null for an account lockout: its text is built at display time from
   * `errorCode` and `lockedUntil` (`sessionErrorText`), so it follows the app language.
   */
  error: string | null;
  errorCode: string | null;
  /** End of the account lockout (UTC RFC 3339 from the server) when `errorCode` is ACCOUNT_LOCKED. */
  lockedUntil: string | null;
  /** True after the first bootstrap refresh has settled (success or failure). */
  hasBootstrapped: boolean;
  /**
   * Raised each time a session becomes usable (sign-in, unlock, password change, re-login) and
   * once after an authentication race (a command refused while the session was in fact valid).
   * Permissions and the dashboard reload when it changes.
   */
  sessionGeneration: number;
  /** The generation raised by the last authentication race (one reload per race, no loop). */
  raceGeneration: number | null;
}

export interface SessionStoreActions {
  refresh: () => Promise<void>;
  login: (req: LoginRequest) => Promise<void>;
  logout: () => Promise<void>;
  unlock: (password: string) => Promise<void>;
  changePassword: (newPassword: string) => Promise<void>;
  /** Session info read outside the actions (silent refresh after a refused command). */
  applySessionInfo: (info: SessionInfo) => void;
  /**
   * Too many wrong passwords at unlock or step-up locked the account (`ACCOUNT_LOCKED`): the
   * server ended the session. Back to sign-in, which shows the same lockout message as a
   * sign-in refusal. `lockedUntil` is the end the server sent (`lockoutUntil`), null when absent.
   */
  applyAccountLockout: (lockedUntil: string | null) => void;
  /**
   * A command was refused as unauthenticated but the session is valid: raise the generation once
   * so the screens that failed load again. A second race before a new session changes nothing.
   */
  noteAuthRace: () => void;
  /** Idempotent first-load of session info (shared across all useSession callers). */
  ensureBootstrapped: () => Promise<void>;
  /** Test/helper: reset store to pre-bootstrap state. */
  resetForTests: () => void;
}

export type SessionStore = SessionStoreState & SessionStoreActions;

let bootstrapInFlight: Promise<void> | null = null;

/**
 * Single app-wide session SSOT. AuthGuard, PermissionProvider, TopBar, etc.
 * must share this — independent useState per hook was wiping nav when
 * PermissionProvider raced its own get_session_info against AuthGuard.
 */
export const useSessionStore = create<SessionStore>((set, get) => ({
  info: null,
  isLoading: true,
  error: null,
  errorCode: null,
  lockedUntil: null,
  hasBootstrapped: false,
  sessionGeneration: 0,
  raceGeneration: null,

  applySessionInfo: (info: SessionInfo): void => {
    set((s) => ({
      info,
      isLoading: false,
      error: null,
      errorCode: null,
      lockedUntil: null,
      hasBootstrapped: true,
      sessionGeneration: generationAfter(s.info, info, s.sessionGeneration),
    }));
  },

  applyAccountLockout: (lockedUntil: string | null): void => {
    clearPermissionCache();
    set({
      info: UNAUTHENTICATED_SESSION,
      isLoading: false,
      error: null,
      errorCode: ACCOUNT_LOCKED,
      lockedUntil,
      hasBootstrapped: true,
    });
  },

  noteAuthRace: (): void => {
    set((s) =>
      s.raceGeneration === s.sessionGeneration
        ? {}
        : { sessionGeneration: s.sessionGeneration + 1, raceGeneration: s.sessionGeneration + 1 },
    );
  },

  ensureBootstrapped: async (): Promise<void> => {
    if (get().hasBootstrapped) {
      return;
    }
    if (bootstrapInFlight) {
      await bootstrapInFlight;
      return;
    }
    bootstrapInFlight = get()
      .refresh()
      .finally(() => {
        bootstrapInFlight = null;
      });
    await bootstrapInFlight;
  },

  refresh: async (): Promise<void> => {
    set({ isLoading: true, error: null, errorCode: null });
    try {
      const info = await getSessionInfo();
      applyAuthenticatedSideEffects(info);
      get().applySessionInfo(info);
    } catch (e) {
      const { message, code } = extractTauriError(e, "Erreur de session.");
      set({
        info: UNAUTHENTICATED_SESSION,
        isLoading: false,
        error: message,
        errorCode: code,
        lockedUntil: code === ACCOUNT_LOCKED ? lockoutUntil(e) : null,
        hasBootstrapped: true,
      });
    }
  },

  login: async (req: LoginRequest): Promise<void> => {
    set({ isLoading: true, error: null, errorCode: null });
    try {
      const response = await authLogin(req);
      applyAuthenticatedSideEffects(response.session_info);
      set((s) => ({
        info: response.session_info,
        isLoading: false,
        error: null,
        errorCode: null,
        lockedUntil: null,
        hasBootstrapped: true,
        sessionGeneration: s.sessionGeneration + 1,
      }));
    } catch (e) {
      const { message, code } = extractTauriError(e, "Identifiant ou mot de passe invalide.");
      const locked = code === ACCOUNT_LOCKED;
      set({
        isLoading: false,
        // A lockout keeps its end, not the server's English sentence (`sessionErrorText`).
        error: locked ? null : message,
        errorCode: code,
        lockedUntil: locked ? lockoutUntil(e) : null,
      });
      throw e;
    }
  },

  logout: async (): Promise<void> => {
    set({ isLoading: true });
    try {
      await authLogout();
      clearPermissionCache();
      set({
        info: UNAUTHENTICATED_SESSION,
        isLoading: false,
        error: null,
        errorCode: null,
        lockedUntil: null,
        hasBootstrapped: true,
      });
    } catch (e) {
      const { message } = extractTauriError(e, "Erreur lors de la déconnexion.");
      set({
        isLoading: false,
        error: message,
        errorCode: null,
        lockedUntil: null,
      });
    }
  },

  unlock: async (password: string): Promise<void> => {
    set({ isLoading: true, error: null, errorCode: null });
    try {
      const info = await authUnlock(password);
      applyAuthenticatedSideEffects(info);
      set((s) => ({
        info,
        isLoading: false,
        error: null,
        errorCode: null,
        lockedUntil: null,
        hasBootstrapped: true,
        sessionGeneration: s.sessionGeneration + 1,
      }));
    } catch (e) {
      const { message, code } = extractTauriError(e, "Échec du déverrouillage.");
      if (code === ACCOUNT_LOCKED) {
        // The server ended the session with the lockout: sign-in shows why.
        const until = lockoutUntil(e);
        get().applyAccountLockout(until);
        throw new Error(accountLockedText(until));
      }
      set({
        isLoading: false,
        error: message,
        errorCode: null,
        lockedUntil: null,
      });
      throw new Error(message);
    }
  },

  changePassword: async (newPassword: string): Promise<void> => {
    set({ isLoading: true, error: null, errorCode: null });
    try {
      const info = await authForceChange(newPassword);
      applyAuthenticatedSideEffects(info);
      set((s) => ({
        info,
        isLoading: false,
        error: null,
        errorCode: null,
        lockedUntil: null,
        hasBootstrapped: true,
        sessionGeneration: s.sessionGeneration + 1,
      }));
    } catch (e) {
      const { message } = extractTauriError(e, "Échec du changement de mot de passe.");
      // The server decides whether the change is still required (it may have been made on
      // another device): take its session state, so the shell leaves the screen when it is not.
      const fresh = await getSessionInfo().catch(() => null);
      set({
        ...(fresh ? { info: fresh } : {}),
        isLoading: false,
        error: message,
        errorCode: null,
        lockedUntil: null,
      });
      throw new Error(message);
    }
  },

  resetForTests: (): void => {
    bootstrapInFlight = null;
    set({
      info: null,
      isLoading: true,
      error: null,
      errorCode: null,
      lockedUntil: null,
      hasBootstrapped: false,
      sessionGeneration: 0,
      raceGeneration: null,
    });
  },
}));
