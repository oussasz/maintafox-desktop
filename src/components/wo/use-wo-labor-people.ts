/**
 * People a labour entry can be booked to: every active person of the register, with or
 * without a login — contractor technicians included (labour references the person since
 * migration 190). Option value = personnel id.
 */
import { useEffect, useState } from "react";

import { type WoLaborPerson, listWoLaborPeople } from "@/services/wo-execution-service";

export interface WoLaborPersonOption {
  /** `personnel.id` as a string (Select value). */
  value: string;
  label: string;
}

export function woLaborPersonLabel(p: WoLaborPerson): string {
  const name = p.full_name.trim() || p.employee_code;
  const base = p.employee_code && name !== p.employee_code ? `${name} (${p.employee_code})` : name;
  return p.company_name ? `${base} · ${p.company_name}` : base;
}

/** Adds the people already booked (and no longer listed) so their entries stay readable. */
export function mergeLaborPeople(
  base: WoLaborPersonOption[],
  extra: readonly WoLaborPersonOption[],
): WoLaborPersonOption[] {
  const known = new Set(base.map((o) => o.value));
  const merged = [...base];
  for (const opt of extra) {
    if (!opt.value || known.has(opt.value)) continue;
    known.add(opt.value);
    merged.push(opt);
  }
  return merged.sort((a, b) => a.label.localeCompare(b.label));
}

export function useWoLaborPeople(): {
  options: WoLaborPersonOption[];
  loading: boolean;
  failed: boolean;
} {
  const [options, setOptions] = useState<WoLaborPersonOption[]>([]);
  const [loading, setLoading] = useState(true);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    listWoLaborPeople()
      .then((rows) => {
        if (cancelled) return;
        setOptions(
          rows
            .map((p) => ({ value: String(p.personnel_id), label: woLaborPersonLabel(p) }))
            .sort((a, b) => a.label.localeCompare(b.label)),
        );
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
