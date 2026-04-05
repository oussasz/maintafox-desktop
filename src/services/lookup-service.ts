// ADR-003: all invoke() calls live exclusively in src/services/.

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";

import type {
  LookupDomainSummary,
  LookupValueOption,
  LookupValueRecord,
  LookupDomainFilter,
  Page,
  PageRequest,
} from "@shared/ipc-types";

// ── Zod schemas ────────────────────────────────────────────────────────────

export const LookupValueOptionSchema = z.object({
  id: z.number().int(),
  code: z.string().min(1),
  label: z.string(),
  fr_label: z.string().nullable(),
  en_label: z.string().nullable(),
  color: z.string().nullable(),
  is_active: z.number().int(),
});

export const LookupValueRecordSchema = LookupValueOptionSchema.extend({
  sync_id: z.string().min(1),
  domain_id: z.number().int(),
  description: z.string().nullable(),
  sort_order: z.number().int(),
  is_system: z.number().int(),
  parent_value_id: z.number().int().nullable(),
});

// ── Service functions ──────────────────────────────────────────────────────

/**
 * Returns all active values for a domain key.
 * This is the primary call for dropdown population across all modules.
 *
 * @param domainKey - the stable domain key, e.g. "equipment.criticality"
 */
export async function getLookupValues(domainKey: string): Promise<LookupValueOption[]> {
  const raw = await invoke<unknown[]>("get_lookup_values", {
    domainKey,
  });
  return z.array(LookupValueOptionSchema).parse(raw);
}

/**
 * Resolves a stored integer FK to a full lookup value record.
 * Use this when rendering a stored id as a labeled badge.
 */
export async function getLookupValueById(valueId: number): Promise<LookupValueRecord> {
  const raw = await invoke<unknown>("get_lookup_value_by_id", {
    valueId,
  });
  return LookupValueRecordSchema.parse(raw) as LookupValueRecord;
}

/**
 * Lists all lookup domains (paginated). Used by the Lookup Manager admin page.
 */
export async function listLookupDomains(
  filter?: LookupDomainFilter,
  page?: PageRequest,
): Promise<Page<LookupDomainSummary>> {
  return invoke<Page<LookupDomainSummary>>("list_lookup_domains", {
    filter: filter ?? null,
    page: page ?? null,
  });
}
