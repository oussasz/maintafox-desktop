import { z } from "zod";

/** Mirrors Rust `SyncHealthSeverityV1`. */
export const SyncHealthSeverityV1Schema = z.enum(["info", "warn", "critical"]);
export type SyncHealthSeverityV1 = z.infer<typeof SyncHealthSeverityV1Schema>;

export const RepairQueueActionV1Schema = z.enum([
  "replay",
  "requeue",
  "acknowledge",
  "escalate",
]);

export const TenantSyncHealthRowV1Schema = z.object({
  tenant_id: z.string().min(1),
  lag_seconds: z.number().nonnegative(),
  checkpoint_age_seconds: z.number().nonnegative(),
  rejection_rate_bps: z.number().nonnegative(),
  retry_pressure: z.number().min(0).max(100),
  dead_letter_count: z.number().nonnegative(),
  severity: SyncHealthSeverityV1Schema,
});

export const SyncFailureDrillDownRowV1Schema = z.object({
  batch_id: z.string().min(1),
  entity_type: z.string().min(1),
  failure_reason_code: z.string().min(1),
  idempotency_key: z.string().min(1),
  last_attempt_rfc3339: z.string(),
  attempt_count: z.number().int().nonnegative(),
});

export const RepairQueueItemV1Schema = z.object({
  item_id: z.string().min(1),
  tenant_id: z.string().min(1),
  queue_kind: z.string().min(1),
  severity: SyncHealthSeverityV1Schema,
  summary: z.string(),
  recommended_action: RepairQueueActionV1Schema,
});

export const OpsAlertStateV1Schema = z.enum(["open", "acknowledged", "resolved"]);

export const IncidentDrillThroughRefsV1Schema = z
  .object({
    tenant_id_hint: z.string().optional(),
    sync_batch_id: z.string().optional(),
    rollout_release_id: z.string().optional(),
    correlation_id: z.string().optional(),
  })
  .strict();

/** Client guard mirroring Rust `tenant_safe_drill_through`. */
export function tenantSafeDrillThrough(refs: z.infer<typeof IncidentDrillThroughRefsV1Schema>): boolean {
  const fields = [
    refs.tenant_id_hint,
    refs.sync_batch_id,
    refs.rollout_release_id,
    refs.correlation_id,
  ];
  for (const s of fields) {
    if (s === undefined) continue;
    const lower = s.toLowerCase();
    if (
      lower.includes("@") ||
      lower.includes("email") ||
      lower.includes("phone") ||
      lower.includes("http://") ||
      lower.includes("https://")
    ) {
      return false;
    }
  }
  return true;
}

export function severityRank(s: SyncHealthSeverityV1): number {
  switch (s) {
    case "info":
      return 0;
    case "warn":
      return 1;
    case "critical":
      return 2;
    default:
      return 0;
  }
}

export function worstSeverity(a: SyncHealthSeverityV1, b: SyncHealthSeverityV1): SyncHealthSeverityV1 {
  return severityRank(a) >= severityRank(b) ? a : b;
}

/** Mirrors Rust `repair_action_allowed` (UX hint; server enforces). */
export function repairActionAllowed(
  item: z.infer<typeof RepairQueueItemV1Schema>,
  action: z.infer<typeof RepairQueueActionV1Schema>,
): boolean {
  const deadLetter = item.queue_kind.includes("dead") || item.queue_kind === "dead_letter";
  switch (action) {
    case "escalate":
      return item.severity === "warn" || item.severity === "critical";
    case "replay":
    case "requeue":
      return true;
    case "acknowledge":
      return item.severity !== "critical" || deadLetter;
    default:
      return false;
  }
}
