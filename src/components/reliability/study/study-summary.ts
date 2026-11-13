/**
 * Plain summary of a study's failure data (UX L2-37): how many failures are used, the machine
 * and the failure mode that weigh most, and the downtime — the "why does it drop" a plant
 * manager reads before the expert steps.
 */

export interface SummaryRecord {
  included: boolean;
  kind: string;
  asset_code: string | null;
  failure_mode_label: string | null;
  downtime_hours: number | null;
}

export interface StudySummary {
  failures: number;
  downtimeHours: number;
  topAsset: { code: string; count: number } | null;
  topMode: { label: string; count: number } | null;
}

function top(counts: Map<string, number>): { key: string; count: number } | null {
  let best: { key: string; count: number } | null = null;
  for (const [key, count] of counts) {
    if (!best || count > best.count || (count === best.count && key < best.key)) {
      best = { key, count };
    }
  }
  return best;
}

export function summarizeStudy(records: readonly SummaryRecord[]): StudySummary {
  const used = records.filter((r) => r.included && r.kind === "failure");
  const byAsset = new Map<string, number>();
  const byMode = new Map<string, number>();
  let downtimeHours = 0;
  for (const r of used) {
    if (r.asset_code) byAsset.set(r.asset_code, (byAsset.get(r.asset_code) ?? 0) + 1);
    if (r.failure_mode_label) {
      byMode.set(r.failure_mode_label, (byMode.get(r.failure_mode_label) ?? 0) + 1);
    }
    downtimeHours += Math.max(0, r.downtime_hours ?? 0);
  }
  const asset = top(byAsset);
  const mode = top(byMode);
  return {
    failures: used.length,
    downtimeHours,
    topAsset: asset ? { code: asset.key, count: asset.count } : null,
    topMode: mode ? { label: mode.key, count: mode.count } : null,
  };
}
