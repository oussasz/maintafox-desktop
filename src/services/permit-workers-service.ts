// ADR-003: invoke() calls for the workers of a work permit, their induction and qualifications
// (L4-05 / L4-06) and their personal padlocks on its group lock box (L4-18). Every rule is judged
// by the server (`permit::workers`): the screen shows its verdict.

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const IssueBlockerSchema = z.object({
  /** A `GATE_` code translated under `errors:gateError`. */
  code: z.string(),
  params: z.record(z.string(), z.string()),
});

const PermitPersonalLockSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  permit_id: z.number(),
  worker_id: z.number(),
  lock_number: z.string(),
  placed_by_id: z.number(),
  placed_by_name: z.string().nullable(),
  placed_at: z.string(),
  removed_by_id: z.number().nullable(),
  removed_by_name: z.string().nullable(),
  removed_at: z.string().nullable(),
  forced_removal: z.boolean(),
  forced_reason: z.string().nullable(),
  row_version: z.number(),
});

const PermitWorkerSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  permit_id: z.number(),
  personnel_id: z.number().nullable(),
  personnel_name: z.string().nullable(),
  employee_code: z.string().nullable(),
  external_name: z.string().nullable(),
  external_company_id: z.number().nullable(),
  company_name: z.string().nullable(),
  /** External staff: an induction is required before the issue. */
  is_external: z.boolean(),
  role_on_permit: z.string(),
  induction_at: z.string().nullable(),
  induction_by_id: z.number().nullable(),
  induction_by_name: z.string().nullable(),
  added_by_id: z.number(),
  added_by_name: z.string().nullable(),
  added_at: z.string(),
  withdrawn_at: z.string().nullable(),
  row_version: z.number(),
  /** The signed-in user is this worker. */
  is_me: z.boolean(),
  open_lock: PermitPersonalLockSchema.nullable(),
  /** What the worker lacks for the permit today (server verdict). */
  blockers: z.array(IssueBlockerSchema),
});

const PermitWorkersViewSchema = z.object({
  permit_id: z.number(),
  permit_status: z.string(),
  workers: z.array(PermitWorkerSchema),
  locks: z.array(PermitPersonalLockSchema),
  open_lock_count: z.number(),
});

export type PermitPersonalLock = z.infer<typeof PermitPersonalLockSchema>;
export type PermitWorker = z.infer<typeof PermitWorkerSchema>;
export type PermitWorkersView = z.infer<typeof PermitWorkersViewSchema>;

/** A person from the personnel list, or an external worker by name and company. */
export interface PermitWorkerAddInput {
  permit_id: number;
  personnel_id: number | null;
  external_name: string | null;
  external_company_id: number | null;
  role_on_permit: string;
  /** RFC 3339: the induction given now by the signed-in user (external staff). */
  induction_at: string | null;
}

export async function listPermitWorkers(permitId: number): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("list_permit_workers", { permitId });
  return PermitWorkersViewSchema.parse(raw);
}

export async function addPermitWorker(input: PermitWorkerAddInput): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("add_permit_worker", { input });
  return PermitWorkersViewSchema.parse(raw);
}

export async function recordPermitWorkerInduction(input: {
  worker_id: number;
  expected_row_version: number;
  induction_at: string | null;
}): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("record_permit_worker_induction", { input });
  return PermitWorkersViewSchema.parse(raw);
}

export async function withdrawPermitWorker(input: {
  worker_id: number;
  expected_row_version: number;
}): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("withdraw_permit_worker", { input });
  return PermitWorkersViewSchema.parse(raw);
}

/** The signed-in user's own padlock (never sent for someone else: the server takes the session). */
export async function placeMyPermitLock(input: {
  permit_id: number;
  lock_number: string;
}): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("place_my_permit_lock", { input });
  return PermitWorkersViewSchema.parse(raw);
}

export async function removeMyPermitLock(input: {
  lock_id: number;
  expected_row_version: number;
}): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("remove_my_permit_lock", { input });
  return PermitWorkersViewSchema.parse(raw);
}

export async function forceRemovePermitLock(input: {
  lock_id: number;
  expected_row_version: number;
  reason: string;
  worker_absent_attested: boolean;
}): Promise<PermitWorkersView> {
  const raw = await invoke<unknown>("force_remove_permit_lock", { input });
  return PermitWorkersViewSchema.parse(raw);
}
