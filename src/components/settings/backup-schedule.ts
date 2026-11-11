/**
 * Automatic backup frequencies in plain words (L7-03): the policy stores a schedule expression
 * (`schedule_cron`, read by the backend scheduler in local time); the screen offers these choices
 * and shows any other stored expression as "custom" without changing it.
 */
export const BACKUP_SCHEDULE_PRESETS = [
  { cron: "0 2 * * *", key: "daily2" },
  { cron: "0 12 * * *", key: "daily12" },
  { cron: "0 20 * * *", key: "daily20" },
  { cron: "0 */6 * * *", key: "every6h" },
  { cron: "0 2 * * 1", key: "weeklyMonday" },
] as const;

type Translate = (key: string, opts?: Record<string, unknown>) => string;

/** The preset key of a stored expression, or null for any other expression. */
export function backupSchedulePreset(expr: string): string | null {
  const normalized = expr.trim().replace(/\s+/g, " ");
  return BACKUP_SCHEDULE_PRESETS.find((p) => p.cron === normalized)?.key ?? null;
}

/** A stored expression in words ("Chaque nuit à 02:00", or "Personnalisée : …"). */
export function describeBackupSchedule(expr: string, t: Translate): string {
  if (!expr.trim()) return t("policy.schedule.none");
  const preset = backupSchedulePreset(expr);
  return preset
    ? t(`policy.schedule.${preset}`)
    : t("policy.schedule.custom", { expr: expr.trim() });
}
