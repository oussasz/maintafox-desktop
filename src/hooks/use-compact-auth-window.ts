import { useEffect } from "react";

import { invoke } from "@/lib/ipc-invoke";

/** Window shapes of the desktop shell (the backend owns the sizes). */
export type WindowMode = "compact" | "main";

/** Moving between two pre-login screens unmounts one shell and mounts the next: no flicker. */
const RELEASE_DELAY_MS = 250;

let current: WindowMode | null = null;
let releaseTimer: ReturnType<typeof setTimeout> | null = null;

async function applyMode(mode: WindowMode): Promise<void> {
  if (current === mode) return;
  current = mode;
  try {
    await invoke<void>("set_window_mode", { mode });
  } catch (err) {
    // Window size is cosmetic: the screens work at any size.
    current = null;
    console.warn("set_window_mode failed", err);
  }
}

/**
 * Pre-login screens (activation, administrator setup, sign-in) use a compact, fixed-size window
 * like an installer; leaving them restores the main window at its remembered size.
 */
export function useCompactAuthWindow(): void {
  useEffect(() => {
    if (releaseTimer != null) {
      clearTimeout(releaseTimer);
      releaseTimer = null;
    }
    void applyMode("compact");
    return () => {
      releaseTimer = setTimeout(() => {
        releaseTimer = null;
        void applyMode("main");
      }, RELEASE_DELAY_MS);
    };
  }, []);
}
