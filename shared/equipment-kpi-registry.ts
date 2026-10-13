import type { OverviewKpiVizKind } from "@shared/overview-kpi-registry";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

/** Semantic keys for Equipment registry strip (`get_equipment_registry_metrics`). */
export const EQUIPMENT_KPI_SEMANTIC_KEYS = {
  EQUIP_STATUS_SUMMARY: "EQUIP_STATUS_SUMMARY",
  EQUIP_CRITICAL_AT_RISK: "EQUIP_CRITICAL_AT_RISK",
  EQUIP_HEALTH_SCORE: "EQUIP_HEALTH_SCORE",
  EQUIP_OPEN_WORK: "EQUIP_OPEN_WORK",
  EQUIP_STALE_METERS: "EQUIP_STALE_METERS",
} as const;

export type EquipmentKpiSemanticKey =
  (typeof EQUIPMENT_KPI_SEMANTIC_KEYS)[keyof typeof EQUIPMENT_KPI_SEMANTIC_KEYS];

export const EQUIPMENT_KPI_CATALOG_IDS = {
  EQUIP_STATUS_SUMMARY: "KPI-047",
  EQUIP_CRITICAL_AT_RISK: "KPI-048",
  EQUIP_HEALTH_SCORE: "KPI-049",
  EQUIP_OPEN_WORK: "KPI-050",
  EQUIP_STALE_METERS: "KPI-051",
} as const;

export interface EquipmentKpiRegistryEntry {
  semanticKey: EquipmentKpiSemanticKey;
  catalogId: string;
  permission: PermissionName;
  trendSense: "up-good" | "up-bad" | "neutral";
  valueKind: "count" | "percent";
  vizKind: OverviewKpiVizKind;
  labelKey: string;
  contextKey: string;
  /** Filter action hint for AssetRegistryPage click-to-filter (Phase 2 UI). */
  drillFilter: "status_down" | "critical_at_risk" | "open_work" | "stale_meters" | "none";
}

export const EQUIPMENT_KPI_REGISTRY: EquipmentKpiRegistryEntry[] = [
  {
    semanticKey: EQUIPMENT_KPI_SEMANTIC_KEYS.EQUIP_STATUS_SUMMARY,
    catalogId: EQUIPMENT_KPI_CATALOG_IDS.EQUIP_STATUS_SUMMARY,
    permission: P.EQ_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    labelKey: "metrics.statusSummary",
    contextKey: "metrics.contextStatus",
    drillFilter: "status_down",
  },
  {
    semanticKey: EQUIPMENT_KPI_SEMANTIC_KEYS.EQUIP_CRITICAL_AT_RISK,
    catalogId: EQUIPMENT_KPI_CATALOG_IDS.EQUIP_CRITICAL_AT_RISK,
    permission: P.EQ_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    labelKey: "metrics.criticalAtRisk",
    contextKey: "metrics.contextCriticalRisk",
    drillFilter: "critical_at_risk",
  },
  {
    semanticKey: EQUIPMENT_KPI_SEMANTIC_KEYS.EQUIP_HEALTH_SCORE,
    catalogId: EQUIPMENT_KPI_CATALOG_IDS.EQUIP_HEALTH_SCORE,
    permission: P.EQ_VIEW,
    trendSense: "up-good",
    valueKind: "percent",
    vizKind: "gauge",
    labelKey: "metrics.healthScore",
    contextKey: "metrics.contextHealth",
    drillFilter: "none",
  },
  {
    semanticKey: EQUIPMENT_KPI_SEMANTIC_KEYS.EQUIP_OPEN_WORK,
    catalogId: EQUIPMENT_KPI_CATALOG_IDS.EQUIP_OPEN_WORK,
    permission: P.EQ_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "histogram",
    labelKey: "metrics.openWork",
    contextKey: "metrics.contextOpenWork",
    drillFilter: "open_work",
  },
  {
    semanticKey: EQUIPMENT_KPI_SEMANTIC_KEYS.EQUIP_STALE_METERS,
    catalogId: EQUIPMENT_KPI_CATALOG_IDS.EQUIP_STALE_METERS,
    permission: P.EQ_VIEW,
    trendSense: "up-bad",
    valueKind: "count",
    vizKind: "alert",
    labelKey: "metrics.staleMeters",
    contextKey: "metrics.contextStaleMeters",
    drillFilter: "stale_meters",
  },
];

export const EQUIPMENT_KPI_REGISTRY_BY_KEY = new Map(
  EQUIPMENT_KPI_REGISTRY.map((e) => [e.semanticKey, e]),
);
