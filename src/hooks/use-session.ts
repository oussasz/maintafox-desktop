import { useState, useCallback, useEffect } from "react";

import { getSessionInfo, login as authLogin, logout as authLogout } from "@/services/auth-service";
import type { SessionInfo, LoginRequest } from "@shared/ipc-types";

interface SessionState {
  info: SessionInfo | null;
  isLoading: boolean;
  error: string | null;
}

interface SessionActions {
  login: (req: LoginRequest) => Promise<void>;
  logout: () => Promise<void>;
  refresh: () => Promise<void>;
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
  });

  const refresh = useCallback(async () => {
    setState((s) => ({ ...s, isLoading: true, error: null }));
    try {
      const info = await getSessionInfo();
      setState({ info, isLoading: false, error: null });
    } catch (e) {
      setState({
        info: UNAUTHENTICATED,
        isLoading: false,
        error: e instanceof Error ? e.message : "Erreur de session.",
      });
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const login = useCallback(async (req: LoginRequest) => {
    setState((s) => ({ ...s, isLoading: true, error: null }));
    try {
      const response = await authLogin(req);
      setState({ info: response.session_info, isLoading: false, error: null });
    } catch (e) {
      setState((s) => ({
        ...s,
        isLoading: false,
        error: e instanceof Error ? e.message : "Identifiant ou mot de passe invalide.",
      }));
      throw e; // re-throw so the login form can react
    }
  }, []);

  const logoutAction = useCallback(async () => {
    setState((s) => ({ ...s, isLoading: true }));
    try {
      await authLogout();
      setState({ info: UNAUTHENTICATED, isLoading: false, error: null });
    } catch (e) {
      setState((s) => ({
        ...s,
        isLoading: false,
        error: e instanceof Error ? e.message : "Erreur lors de la déconnexion.",
      }));
    }
  }, []);

  return { ...state, login, logout: logoutAction, refresh };
}
