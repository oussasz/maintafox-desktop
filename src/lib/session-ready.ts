import { useSessionStore } from "@/store/session-store";

/**
 * True when background polls / heartbeats may call protected IPC.
 * Idle-locked and unauthenticated sessions must not trigger AuthLock via polls.
 */
export function isSessionActiveForBackgroundWork(): boolean {
  const info = useSessionStore.getState().info;
  return info?.is_authenticated === true && !info.is_locked;
}
