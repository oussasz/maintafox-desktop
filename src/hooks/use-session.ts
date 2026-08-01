import { useEffect } from "react";

import { useSessionStore, type SessionStore } from "@/store/session-store";

/**
 * Primary session hook. Backed by a shared Zustand store so AuthGuard,
 * PermissionProvider, TopBar, and forms all see the same auth state.
 *
 * Independent per-component useState previously caused PermissionProvider to
 * start unauthenticated (empty nav) even while AuthGuard already had a session.
 */
export function useSession(): SessionStore {
  const store = useSessionStore();

  useEffect(() => {
    void useSessionStore.getState().ensureBootstrapped();
  }, []);

  return store;
}
