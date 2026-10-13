/** i18n helpers for PM enums — never show raw snake_case to operators. */

export const PM_STRATEGY_TYPES = ["fixed", "floating", "meter", "event", "condition"] as const;
export const PM_LIFECYCLE_STATUSES = [
  "draft",
  "proposed",
  "approved",
  "active",
  "suspended",
  "retired",
] as const;
export const PM_OCCURRENCE_STATUSES = [
  "forecasted",
  "generated",
  "ready_for_scheduling",
  "scheduled",
  "in_progress",
  "completed",
  "deferred",
  "missed",
  "cancelled",
] as const;
export const PM_ACTIVE_OCCURRENCE_STATUSES = [
  "forecasted",
  "generated",
  "ready_for_scheduling",
  "scheduled",
  "in_progress",
  "deferred",
] as const;
export const PM_SCOPE_TYPES = ["equipment", "family", "location", "criticality_group"] as const;
export const PM_REASON_CODES = [
  "capacity_constraint",
  "asset_unavailable",
  "material_missing",
  "weather_constraint",
  "safety_constraint",
  "other",
] as const;
export const PM_EXECUTION_RESULTS = [
  "completed_no_findings",
  "completed_with_findings",
  "deferred",
  "missed",
  "cancelled",
] as const;

export type PmStrategyType = (typeof PM_STRATEGY_TYPES)[number];
export type PmLifecycleStatus = (typeof PM_LIFECYCLE_STATUSES)[number];
export type PmOccurrenceStatus = (typeof PM_OCCURRENCE_STATUSES)[number];

export function allowedPlanLifecycleTargets(current: string): string[] {
  switch (current) {
    case "draft":
      return ["proposed", "retired"];
    case "proposed":
      return ["draft", "approved", "retired"];
    case "approved":
      return ["active", "suspended", "retired"];
    case "active":
      return ["suspended", "retired"];
    case "suspended":
      return ["active", "retired"];
    default:
      return [];
  }
}

export function allowedOccurrenceTargets(current: string): string[] {
  switch (current) {
    case "forecasted":
      return ["generated", "cancelled", "missed"];
    case "generated":
      return ["ready_for_scheduling", "deferred", "cancelled", "missed"];
    case "ready_for_scheduling":
      return ["scheduled", "deferred", "cancelled"];
    case "scheduled":
      return ["in_progress", "deferred", "cancelled", "missed"];
    case "in_progress":
      return ["completed", "deferred", "cancelled"];
    case "deferred":
      return ["ready_for_scheduling", "scheduled", "missed", "cancelled"];
    case "missed":
      return ["generated", "cancelled"];
    default:
      return [];
  }
}

export const PUBLISHABLE_PLAN_LIFECYCLES = new Set(["approved", "active", "suspended"]);
