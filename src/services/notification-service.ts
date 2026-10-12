import { z } from "zod";

import { invoke, invokeSilent } from "@/lib/ipc-invoke";

const NotificationSummarySchema = z.object({
  id: z.number(),
  title: z.string(),
  body: z.string().nullable(),
  category_code: z.string(),
  severity: z.string(),
  delivery_state: z.string(),
  created_at: z.string(),
  read_at: z.string().nullable(),
  acknowledged_at: z.string().nullable(),
  action_url: z.string().nullable(),
  escalation_level: z.number(),
  requires_ack: z.boolean().default(false),
});

const UserPreferenceRowSchema = z.object({
  category_code: z.string(),
  label: z.string(),
  is_user_configurable: z.boolean(),
  in_app_enabled: z.boolean(),
  os_enabled: z.boolean(),
  email_enabled: z.boolean(),
  sms_enabled: z.boolean(),
  digest_mode: z.string(),
  muted_until: z.string().nullable(),
});

export type NotificationSummary = z.infer<typeof NotificationSummarySchema>;
export type UserPreferenceRow = z.infer<typeof UserPreferenceRowSchema>;

export interface NotificationFilterInput {
  delivery_state?: string;
  category_code?: string;
  limit?: number;
  offset?: number;
}

export interface UpdateNotificationPreferenceInput {
  category_code: string;
  in_app_enabled?: boolean;
  os_enabled?: boolean;
  email_enabled?: boolean;
  sms_enabled?: boolean;
  digest_mode?: string;
  muted_until?: string | null;
}

export async function listNotifications(
  filter: NotificationFilterInput,
): Promise<NotificationSummary[]> {
  const raw = await invoke<NotificationSummary[]>("list_notifications", { filter });
  return z.array(NotificationSummarySchema).parse(raw);
}

export async function getUnreadCount(): Promise<number> {
  return invokeSilent<number>("get_unread_count");
}

export async function markNotificationRead(notification_id: number): Promise<void> {
  await invoke<void>("mark_notification_read", { notification_id });
}

export async function acknowledgeNotification(
  notification_id: number,
  note?: string,
): Promise<void> {
  await invoke<void>("acknowledge_notification", { notification_id, note });
}

export async function snoozeNotification(
  notification_id: number,
  snooze_minutes: number,
): Promise<void> {
  await invoke<void>("snooze_notification", { notification_id, snooze_minutes });
}

export async function getNotificationPreferences(): Promise<UserPreferenceRow[]> {
  const raw = await invoke<UserPreferenceRow[]>("get_notification_preferences");
  return z.array(UserPreferenceRowSchema).parse(raw);
}

export async function updateNotificationPreference(
  payload: UpdateNotificationPreferenceInput,
): Promise<void> {
  await invoke<void>("update_notification_preference", { payload });
}

const OsToastBatchSchema = z.object({
  watermark: z.number(),
  items: z.array(
    z.object({
      notification_id: z.number(),
      title: z.string(),
      body: z.string().nullable(),
      severity: z.string(),
      category_code: z.string(),
    }),
  ),
});

export type OsToastBatch = z.infer<typeof OsToastBatchSchema>;
export type OsToastCandidate = OsToastBatch["items"][number];

/**
 * Notifications to show as an OS toast. Without `sinceId` (window just opened) it returns the
 * watermark to pass on later calls and nothing to show, so signing in does not replay the inbox.
 */
export async function listPendingOsToasts(sinceId: number | null): Promise<OsToastBatch> {
  const raw = await invokeSilent<unknown>("list_pending_os_toasts", { sinceId });
  return OsToastBatchSchema.parse(raw);
}

const ShowOsToastResultSchema = z.object({ shown: z.boolean(), error: z.string().nullable() });

/** Shows one OS toast for these notifications; the backend records the outcome for each. */
export async function showOsToast(input: {
  notification_ids: number[];
  title: string;
  body: string;
}): Promise<z.infer<typeof ShowOsToastResultSchema>> {
  const raw = await invokeSilent<unknown>("show_os_toast", { input });
  return ShowOsToastResultSchema.parse(raw);
}

const ChannelAvailabilitySchema = z.object({
  available: z.boolean(),
  reason: z.string().nullable(),
});
const NotificationChannelStatusSchema = z.object({ email: ChannelAvailabilitySchema });

export type NotificationChannelStatus = z.infer<typeof NotificationChannelStatusSchema>;

/** Whether the email channel can be used on this device (server endpoint, licence, activation). */
export async function getNotificationChannelStatus(): Promise<NotificationChannelStatus> {
  const raw = await invoke<unknown>("get_notification_channel_status");
  return NotificationChannelStatusSchema.parse(raw);
}
