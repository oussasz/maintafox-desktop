import { create } from "zustand";

import type { SessionInfo } from "@shared/ipc-types";

/** `session`: the session is lost (sign in again); `unknown`: no lock open (idle value). */
export type AuthSessionLockMode = "session" | "unknown";

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
 * Global "session lost" interceptor: a single lock screen the user resolves by signing in again.
 * Permission refusals do not come here (one toast with the reason, see `ipc-invoke`).
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
    // Already open: keep the mode unless a session loss replaces an unknown one.
    if (s.isLockOpen) {
      const shouldUpgrade = mode === "session" && s.mode === "unknown";
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
