/**
 * Notification counts shared by the bell and the inbox page, so both always show the numbers the
 * backend computes with the inbox's own conditions (`notifications::inbox::counts`). Anything that
 * changes a notification (read, acknowledge, snooze, archive) calls `refresh()`.
 */

import { create } from "zustand";

import { isSessionActiveForBackgroundWork } from "@/lib/session-ready";
import { type InboxCounts, getNotificationCounts } from "@/services/notification-service";

const EMPTY: InboxCounts = { unread: 0, to_acknowledge: 0, snoozed: 0, by_module: [] };

interface NotificationStoreState {
  counts: InboxCounts;
  /** Increases whenever the unread count went up: a new notification arrived. */
  arrivals: number;
  refresh: () => Promise<void>;
  reset: () => void;
}

export const useNotificationStore = create<NotificationStoreState>()((set, get) => ({
  counts: EMPTY,
  arrivals: 0,
  refresh: async () => {
    if (!isSessionActiveForBackgroundWork()) {
      set({ counts: EMPTY });
      return;
    }
    try {
      const counts = await getNotificationCounts();
      const before = get().counts.unread;
      set({ counts, arrivals: counts.unread > before ? get().arrivals + 1 : get().arrivals });
    } catch {
      // Background poll: a failure keeps the last known counts (the inbox reports its own errors).
    }
  },
  reset: () => set({ counts: EMPTY }),
}));
