/**
 * Shows OS toasts for notifications that arrive while the user is signed in (backend:
 * `notifications::os_toast`). The first call of a session takes a watermark so the inbox that
 * existed at sign-in is not replayed; each later poll toasts what arrived since, once.
 */

import type { TFunction } from "i18next";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import {
  type OsToastCandidate,
  listPendingOsToasts,
  showOsToast,
} from "@/services/notification-service";

export const OS_TOAST_POLL_MS = 30_000;

/** One toast: the notification itself, or a summary when several arrived together. */
export function composeOsToast(
  items: OsToastCandidate[],
  t: TFunction<"notifications">,
): { title: string; body: string } | null {
  const [first, ...rest] = items;
  if (!first) return null;
  if (rest.length === 0) {
    return { title: first.title, body: first.body ?? "" };
  }
  return {
    title: t("osToast.summaryTitle", { count: items.length }),
    body: t("osToast.summaryBody", { count: items.length, first: first.title, rest: rest.length }),
  };
}

/** One poll; returns the watermark to keep. The backend records each toast's outcome. */
export async function pollOsToasts(
  watermark: number | null,
  t: TFunction<"notifications">,
): Promise<number> {
  const batch = await listPendingOsToasts(watermark);
  const toast = composeOsToast(batch.items, t);
  if (toast) {
    await showOsToast({
      notification_ids: batch.items.map((i) => i.notification_id),
      title: toast.title,
      body: toast.body,
    });
  }
  return batch.watermark;
}

export function useOsToasts(active: boolean): void {
  const { t } = useTranslation("notifications");
  const watermark = useRef<number | null>(null);

  useEffect(() => {
    if (!active) {
      watermark.current = null;
      return;
    }
    let cancelled = false;
    const tick = async () => {
      try {
        const next = await pollOsToasts(watermark.current, t);
        if (!cancelled) watermark.current = next;
      } catch (err) {
        // The in-app inbox still lists the notification; the toast is tried again next poll.
        console.warn("os toast poll failed", err);
      }
    };
    void tick();
    const id = window.setInterval(() => void tick(), OS_TOAST_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [active, t]);
}
