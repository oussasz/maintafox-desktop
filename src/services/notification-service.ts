import { z } from "zod";

import { invoke, invokeSilent } from "@/lib/ipc-invoke";

// Tauri passes command arguments by their camelCase name (`notification_id` → `notificationId`).

/** One notification of the inbox (`notifications::inbox::InboxItem`). */
const NotificationSummarySchema = z.object({
  id: z.number(),
  title: z.string(),
  body: z.string().nullable(),
  category_code: z.string(),
  module_code: z.string(),
  /** SYSTEM severity: info | warning | critical. */
  severity: z.string(),
  /** Shared urgency scale level (`WORK.URGENCY`) when the notification is about work. */
  urgency_code: z.string().nullable(),
  message_key: z.string().nullable(),
  message_params: z.record(z.string(), z.unknown()).nullable(),
  delivery_state: z.string(),
  created_at: z.string(),
  read_at: z.string().nullable(),
  acknowledged_at: z.string().nullable(),
  snoozed_until: z.string().nullable(),
  archived_at: z.string().nullable(),
  resolved_at: z.string().nullable(),
  action_url: z.string().nullable(),
  escalation_level: z.number(),
  requires_ack: z.boolean(),
  event_acknowledged_by: z.string().nullable(),
  reason_code: z.string().nullable(),
  reason_ref: z.string().nullable(),
  /** The condition was already true when this device started watching it (listed as read). */
  already_true: z.boolean().default(false),
  source_module: z.string(),
  source_record_id: z.string().nullable(),
});

const InboxCountsSchema = z.object({
  unread: z.number(),
  to_acknowledge: z.number(),
  snoozed: z.number(),
  by_module: z.array(z.object({ module_code: z.string(), unread: z.number() })),
});

const UserPreferenceRowSchema = z.object({
  category_code: z.string(),
  module_code: z.string(),
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
export type InboxCounts = z.infer<typeof InboxCountsSchema>;
export type UserPreferenceRow = z.infer<typeof UserPreferenceRowSchema>;

/** Inbox views (`notifications::inbox::VIEWS`). */
export const INBOX_VIEWS = ["inbox", "unread", "to_acknowledge", "snoozed", "archived"] as const;
export type InboxView = (typeof INBOX_VIEWS)[number];

export interface NotificationFilterInput {
  view?: InboxView | "all";
  module_code?: string;
  severity?: string;
  category_code?: string;
  /** Inclusive RFC 3339 bounds on the creation time. */
  created_from?: string;
  created_to?: string;
  limit?: number;
  offset?: number;
}

export interface UpdateNotificationPreferenceInput {
  category_code: string;
  in_app_enabled?: boolean;
  os_enabled?: boolean;
  email_enabled?: boolean;
  digest_mode?: string;
  muted_until?: string | null;
}

export async function listNotifications(
  filter: NotificationFilterInput,
): Promise<NotificationSummary[]> {
  const raw = await invoke<unknown>("list_notifications", { filter });
  return z.array(NotificationSummarySchema).parse(raw);
}

export async function getUnreadCount(): Promise<number> {
  return invokeSilent<number>("get_unread_count");
}

export async function getNotificationCounts(): Promise<InboxCounts> {
  const raw = await invokeSilent<unknown>("get_notification_counts");
  return InboxCountsSchema.parse(raw);
}

export async function markNotificationRead(notificationId: number): Promise<void> {
  await invoke<void>("mark_notification_read", { notificationId });
}

export async function markNotificationUnread(notificationId: number): Promise<void> {
  await invoke<void>("mark_notification_unread", { notificationId });
}

/** Marks read every unread notification matching the filter; returns how many. */
export async function markAllNotificationsRead(filter: NotificationFilterInput): Promise<number> {
  return invoke<number>("mark_all_notifications_read", { filter });
}

export async function acknowledgeNotification(
  notificationId: number,
  note?: string,
): Promise<void> {
  await invoke<void>("acknowledge_notification", { notificationId, note });
}

/** Snoozes until `snoozeUntil` (within the next 7 days). */
export async function snoozeNotification(notificationId: number, snoozeUntil: Date): Promise<void> {
  await invoke<void>("snooze_notification", {
    notificationId,
    snoozeUntil: snoozeUntil.toISOString(),
  });
}

export async function archiveNotification(notificationId: number): Promise<void> {
  await invoke<void>("archive_notification", { notificationId });
}

export async function unarchiveNotification(notificationId: number): Promise<void> {
  await invoke<void>("unarchive_notification", { notificationId });
}

export async function getNotificationPreferences(): Promise<UserPreferenceRow[]> {
  const raw = await invoke<unknown>("get_notification_preferences");
  return z.array(UserPreferenceRowSchema).parse(raw);
}

export async function updateNotificationPreference(
  payload: UpdateNotificationPreferenceInput,
): Promise<void> {
  await invoke<void>("update_notification_preference", { payload });
}

// ─── Plant settings (Données de référence → Notifications) ─────────────────────

const RuleConfigSchema = z.object({
  rule_id: z.number(),
  category_code: z.string(),
  module_code: z.string(),
  default_severity: z.string(),
  is_user_configurable: z.boolean(),
  fixed_routing: z.boolean(),
  is_active: z.boolean(),
  audiences: z.array(z.string()),
  requires_ack: z.boolean(),
  escalation_policy_id: z.number().nullable(),
  uses_lead_time: z.boolean(),
  lead_time_minutes: z.number().nullable(),
});

const EscalationStepSchema = z.object({
  id: z.number(),
  level_no: z.number(),
  wait_minutes: z.number(),
  target: z.string(),
  reference: z.string().nullable(),
});

const NotificationConfigSchema = z.object({
  rules: z.array(RuleConfigSchema),
  policies: z.array(
    z.object({ id: z.number(), name: z.string(), steps: z.array(EscalationStepSchema) }),
  ),
  retention: z.object({
    expiry_days: z.number(),
    is_default: z.boolean(),
    default_days: z.number(),
    min_days: z.number(),
    max_days: z.number(),
  }),
  audiences: z.array(z.string()),
  escalation_targets: z.array(z.string()),
});

export type NotificationRuleConfig = z.infer<typeof RuleConfigSchema>;
export type NotificationEscalationStep = z.infer<typeof EscalationStepSchema>;
export type NotificationConfig = z.infer<typeof NotificationConfigSchema>;

export interface UpdateNotificationRuleInput {
  rule_id: number;
  is_active?: boolean;
  audiences?: string[];
  requires_ack?: boolean;
  escalation_policy_id?: number | null;
  lead_time_minutes?: number | null;
}

export interface EscalationStepInput {
  wait_minutes: number;
  target: string;
  reference: string | null;
}

export async function getNotificationConfig(): Promise<NotificationConfig> {
  return NotificationConfigSchema.parse(await invoke<unknown>("get_notification_config"));
}

export async function updateNotificationRule(
  payload: UpdateNotificationRuleInput,
): Promise<NotificationConfig> {
  return NotificationConfigSchema.parse(
    await invoke<unknown>("update_notification_rule", { payload }),
  );
}

export async function updateNotificationEscalationStep(
  stepId: number,
  payload: EscalationStepInput,
): Promise<NotificationConfig> {
  return NotificationConfigSchema.parse(
    await invoke<unknown>("update_notification_escalation_step", { stepId, payload }),
  );
}

export async function addNotificationEscalationStep(
  policyId: number,
  payload: EscalationStepInput,
): Promise<NotificationConfig> {
  return NotificationConfigSchema.parse(
    await invoke<unknown>("add_notification_escalation_step", { policyId, payload }),
  );
}

export async function removeNotificationEscalationStep(
  stepId: number,
): Promise<NotificationConfig> {
  return NotificationConfigSchema.parse(
    await invoke<unknown>("remove_notification_escalation_step", { stepId }),
  );
}

export async function setNotificationRetention(expiryDays: number): Promise<NotificationConfig> {
  return NotificationConfigSchema.parse(
    await invoke<unknown>("set_notification_retention", { expiryDays }),
  );
}

// ─── OS toasts and channels ────────────────────────────────────────────────────

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
