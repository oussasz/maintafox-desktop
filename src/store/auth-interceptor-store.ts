import { create } from "zustand";

import type { SessionInfo } from "@shared/ipc-types";

export type AuthSessionLockMode = "session" | "permission" | "unknown";

export interface LastAuthFailure {
  atMs: number;
  command: string;
  code: string | null;
  message: string;
}

export interface AuthInterceptorState {
  isLockOpen: boolean;
  mode: AuthSessionLockMode;
  lastFailure: LastAuthFailure | null;
  /** Best-effort snapshot of the last known good session, used to personalize recovery UI. */
  preInterruptSnapshot: SessionInfo | null;
  openFromAuthFailure: (args: { mode: AuthSessionLockMode; failure: LastAuthFailure }) => void;
  rememberPreInterrupt: (info: SessionInfo) => void;
  clear: () => void;
}

/**
 * Global auth/authorization "interceptor" surface.
 *
 * Unlike toast spam, this is a single modal/lock screen that the user can resolve safely.
 */
export const useAuthInterceptorStore = create<AuthInterceptorState>((set, get) => ({
  isLockOpen: false,
  mode: "unknown",
  lastFailure: null,
  preInterruptSnapshot: null,

  rememberPreInterrupt: (info: SessionInfo): void => {
    if (!info.is_authenticated) {
      return;
    }
    set({ preInterruptSnapshot: info });
  },

  openFromAuthFailure: ({
    mode,
    failure,
  }: {
    mode: AuthSessionLockMode;
    failure: LastAuthFailure;
  }): void => {
    const s = get();
    // If we're already in a stricter/known mode, keep it unless the new one is "session"
    // (auth/session issues should take precedence over generic permission lock).
    if (s.isLockOpen) {
      const shouldUpgrade = mode === "session" && (s.mode === "permission" || s.mode === "unknown");
      if (!shouldUpgrade) {
        set({ lastFailure: failure });
        return;
      }
    }
    set({
      isLockOpen: true,
      mode,
      lastFailure: failure,
    });
  },

  clear: (): void => {
    set({
      isLockOpen: false,
      mode: "unknown",
      lastFailure: null,
      // Keep snapshot: it's still useful if another interruption happens in the same day.
    });
  },
}));
