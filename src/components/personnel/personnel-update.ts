/**
 * Person update: optional fields are sent only when the form changed them. The server reads an
 * absent field as "unchanged" and `null` as "cleared" (`personnel::domain`, Option<Option<_>>).
 */
import type { Personnel, PersonnelUpdateInput } from "@shared/ipc-types";

/** Optional person fields an update can clear (`null`) or leave unchanged (absent). */
type ClearableField =
  | "supervisor_id"
  | "hire_date"
  | "termination_date"
  | "email"
  | "phone"
  | "external_company_id"
  | "contract_number"
  | "contract_start_date"
  | "contract_end_date"
  | "notes";

/**
 * The optional fields whose form value differs from the record: a changed value is sent, an
 * emptied one as `null` (the server clears it), an unchanged one is left out.
 */
export function clearableChanges(
  before: Pick<Personnel, ClearableField>,
  after: Partial<Pick<Personnel, ClearableField>>,
): Partial<Pick<PersonnelUpdateInput, ClearableField>> {
  const out: Partial<Pick<PersonnelUpdateInput, ClearableField>> = {};
  for (const key of Object.keys(after) as ClearableField[]) {
    const next = after[key] ?? null;
    if (next !== (before[key] ?? null)) (out as Record<string, unknown>)[key] = next;
  }
  return out;
}
