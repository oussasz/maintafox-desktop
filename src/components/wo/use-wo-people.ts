/**
 * People selectable on a work order (labour technician, responsible, planner, FMECA
 * override signer): the personnel register's active staff that have a login, since
 * every WO person field references `user_accounts`.
 */
import { useEffect, useState } from "react";

import { type WoLaborCandidate, listWoLaborCandidates } from "@/services/wo-execution-service";

export interface WoPersonOption {
  /** `user_accounts.id` as a string (Select value). */
  value: string;
  label: string;
}

export function woPersonLabel(c: WoLaborCandidate): string {
  const name = c.full_name.trim() || c.employee_code;
  return c.employee_code && name !== c.employee_code ? `${name} (${c.employee_code})` : name;
}

export function toWoPersonOptions(candidates: WoLaborCandidate[]): WoPersonOption[] {
  const seen = new Set<string>();
  const out: WoPersonOption[] = [];
  for (const c of candidates) {
    const value = String(c.user_id);
    if (seen.has(value)) continue;
    seen.add(value);
    out.push({ value, label: woPersonLabel(c) });
  }
  return out.sort((a, b) => a.label.localeCompare(b.label));
}

/** Adds `extra` entries (e.g. the saved value, the current user) missing from `base`. */
export function mergeWoPersonOptions(
  base: WoPersonOption[],
  extra: readonly WoPersonOption[],
): WoPersonOption[] {
  const known = new Set(base.map((o) => o.value));
  const merged = [...base];
  for (const opt of extra) {
    if (!opt.value || known.has(opt.value)) continue;
    known.add(opt.value);
    merged.push(opt);
  }
  return merged.sort((a, b) => a.label.localeCompare(b.label));
}

export function useWoPeopleOptions(): {
  options: WoPersonOption[];
  loading: boolean;
  failed: boolean;
} {
  const [options, setOptions] = useState<WoPersonOption[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    listWoLaborCandidates()
      .then((rows) => {
        if (cancelled) return;
        setOptions(toWoPersonOptions(rows));
        setFailed(false);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return { options, loading, failed };
}
