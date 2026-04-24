import { useState, useCallback, useEffect } from "react";

import {
  getSessionInfo,
  login as authLogin,
  logout as authLogout,
  unlockSession as authUnlock,
  forceChangePassword as authForceChange,
} from "@/services/auth-service";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";
import type { SessionInfo, LoginRequest } from "@shared/ipc-types";

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

interface SessionState {
  info: SessionInfo | null;
  isLoading: boolean;
  error: string | null;
  errorCode: string | null;
}

interface SessionActions {
  login: (req: LoginRequest) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
  unlock: (password: string) => Promise<void>;
  changePassword: (newPassword: string) => Promise<void>;
}

const UNAUTHENTICATED: SessionInfo = {
  is_authenticated: false,
  is_locked: false,
  user_id: null,
  username: null,
  display_name: null,
  is_admin: null,
  force_password_change: null,
  expires_at: null,
  last_activity_at: null,
  password_expires_in_days: null,
  pin_configured: null,
  tenant_id: null,
  token_tenant_id: null,
};

/**
 * Primary session hook. Fetches session state on mount and after login/logout.
 * Components that need to gate on authentication status use this hook.
 */
export function useSession(): SessionState & SessionActions {
  const [state, setState] = useState<SessionState>({
    info: null,
    isLoading: true,
    error: null,
    errorCode: null,
  });

  const refresh = useCallback(async () => {
    setState((s) => ({ ...s, isLoading: true, error: null, errorCode: null }));
    try {
      const info = await getSessionInfo();
      if (info.is_authenticated) {
        useAuthInterceptorStore.getState().rememberPreInterrupt(info);
        if (useAuthInterceptorStore.getState().isLockOpen) {
          useAuthInterceptorStore.getState().clear();
        }
      }
      setState({ info, isLoading: false, error: null, errorCode: null });
    } catch (e) {
      const { message, code } = extractTauriError(e, "Erreur de session.");
      setState({
        info: UNAUTHENTICATED,
        isLoading: false,
        error: message,
        errorCode: code,
      });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const login = useCallback(async (req: LoginRequest) => {
    setState((s) => ({ ...s, isLoading: true, error: null, errorCode: null }));
    try {
      const response = await authLogin(req);
      if (response.session_info.is_authenticated) {
        useAuthInterceptorStore.getState().rememberPreInterrupt(response.session_info);
        if (useAuthInterceptorStore.getState().isLockOpen) {
          useAuthInterceptorStore.getState().clear();
        }
      }
      setState({ info: response.session_info, isLoading: false, error: null, errorCode: null });
    } catch (e) {
      const { message, code } = extractTauriError(e, "Identifiant ou mot de passe invalide.");
      setState((s) => ({
        ...s,
        isLoading: false,
        error: message,
        errorCode: code,
      }));
      throw e; // re-throw so the login form can react
    }
  }, []);

  const logoutAction = useCallback(async () => {
    setState((s) => ({ ...s, isLoading: true }));
    try {
      await authLogout();
      setState({ info: UNAUTHENTICATED, isLoading: false, error: null, errorCode: null });
    } catch (e) {
      const { message } = extractTauriError(e, "Erreur lors de la d\u00e9connexion.");
      setState((s) => ({
        ...s,
        isLoading: false,
        error: message,
        errorCode: null,
      }));
    }
  }, []);

  const unlock = useCallback(async (password: string) => {
    setState((s) => ({ ...s, isLoading: true, error: null, errorCode: null }));
    try {
      const info = await authUnlock(password);
      if (info.is_authenticated) {
        useAuthInterceptorStore.getState().rememberPreInterrupt(info);
        if (useAuthInterceptorStore.getState().isLockOpen) {
          useAuthInterceptorStore.getState().clear();
        }
      }
      setState({ info, isLoading: false, error: null, errorCode: null });
    } catch (e) {
      const { message } = extractTauriError(e, "\u00c9chec du d\u00e9verrouillage.");
      setState((s) => ({
        ...s,
        isLoading: false,
        error: message,
        errorCode: null,
      }));
      throw new Error(message);
    }
  }, []);

  const changePassword = useCallback(async (newPassword: string) => {
    setState((s) => ({ ...s, isLoading: true, error: null, errorCode: null }));
    try {
      const info = await authForceChange(newPassword);
      if (info.is_authenticated) {
        useAuthInterceptorStore.getState().rememberPreInterrupt(info);
        if (useAuthInterceptorStore.getState().isLockOpen) {
          useAuthInterceptorStore.getState().clear();
        }
      }
      setState({ info, isLoading: false, error: null, errorCode: null });
    } catch (e) {
      const { message } = extractTauriError(e, "\u00c9chec du changement de mot de passe.");
      setState((s) => ({
        ...s,
        isLoading: false,
        error: message,
        errorCode: null,
      }));
      throw new Error(message);
    }
  }, []);

  return { ...state, login, logout: logoutAction, refresh, unlock, changePassword };
}
