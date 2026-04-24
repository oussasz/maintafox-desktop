/**
 * org-node-service.ts
 *
 * IPC wrappers for org node lifecycle, responsibility bindings, and entity
 * bindings. All invoke() calls for F02 node-level commands are isolated here.
 *
 * Config-layer commands (structure models, node types, relationship rules)
 * remain in org-service.ts (SP01-F01).
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import type {
  AssignEquipmentPayload,
  AssignResponsibilityPayload,
  CreateOrgNodePayload,
  MoveOrgNodePayload,
  OrgEntityBinding,
  OrgNode,
  OrgNodeEquipmentRow,
  OrgNodeResponsibility,
  OrgTreeRow,
  UpdateOrgNodeMetadataPayload,
  UpsertOrgEntityBindingPayload,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export const OrgNodeSchema = z.object({
  id: z.number(),
  sync_id: z.string(),
  code: z.string(),
  name: z.string(),
  node_type_id: z.number(),
  parent_id: z.number().nullable(),
  ancestor_path: z.string(),
  depth: z.number(),
  description: z.string().nullable(),
  cost_center_code: z.string().nullable(),
  external_reference: z.string().nullable(),
  status: z.string(),
  effective_from: z.string().nullable(),
  effective_to: z.string().nullable(),
  erp_reference: z.string().nullable(),
  notes: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  deleted_at: z.string().nullable(),
  row_version: z.number(),
  origin_machine_id: z.string().nullable(),
  last_synced_checkpoint: z.string().nullable(),
});

export const OrgTreeRowSchema = z.object({
  node: OrgNodeSchema,
  node_type_code: z.string(),
  node_type_label: z.string(),
  can_host_assets: z.boolean(),
  can_own_work: z.boolean(),
  can_carry_cost_center: z.boolean(),
  can_aggregate_kpis: z.boolean(),
  can_receive_permits: z.boolean(),
  child_count: z.number(),
});

export const OrgNodeResponsibilitySchema = z.object({
  id: z.number(),
  node_id: z.number(),
  responsibility_type: z.string(),
  person_id: z.number().nullable(),
  team_id: z.number().nullable(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const OrgEntityBindingSchema = z.object({
  id: z.number(),
  node_id: z.number(),
  binding_type: z.string(),
  external_system: z.string(),
  external_id: z.string(),
  is_primary: z.boolean(),
  valid_from: z.string().nullable(),
  valid_to: z.string().nullable(),
  created_at: z.string(),
});

// â”€â”€ Error helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/**
 * Structured IPC error shape returned by AppError's Serialize impl.
 * Matches the `{ code, message }` JSON produced in errors.rs.
 */
interface IpcError {
  code: string;
  message: string;
}

function isIpcError(err: unknown): err is IpcError {
  return typeof err === "object" && err !== null && "code" in err && "message" in err;
}

/** Custom error class for stale row-version rejections. */
export class VersionConflictError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "VersionConflictError";
  }
}

/**
 * Detect row-version mismatch errors and throw a distinct error class so
 * the UI can prompt a refresh instead of showing a generic failure.
 */
function rethrowIfVersionConflict(err: unknown): never {
  if (
    isIpcError(err) &&
    err.code === "VALIDATION_FAILED" &&
    err.message.includes("row version mismatch")
  ) {
    throw new VersionConflictError(err.message);
  }
  throw err;
}

// â”€â”€ Org node commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listOrgTree(): Promise<OrgTreeRow[]> {
  const raw = await invoke<unknown>("list_org_tree");
  return z.array(OrgTreeRowSchema).parse(raw) as OrgTreeRow[];
}

export async function getOrgNode(nodeId: number): Promise<OrgNode> {
  const raw = await invoke<unknown>("get_org_node", { nodeId });
  return OrgNodeSchema.parse(raw) as OrgNode;
}

export async function createOrgNode(payload: CreateOrgNodePayload): Promise<OrgNode> {
  const raw = await invoke<unknown>("create_org_node", { payload });
  return OrgNodeSchema.parse(raw) as OrgNode;
}

export async function updateOrgNodeMetadata(
  payload: UpdateOrgNodeMetadataPayload,
): Promise<OrgNode> {
  try {
    const raw = await invoke<unknown>("update_org_node_metadata", { payload });
    return OrgNodeSchema.parse(raw) as OrgNode;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function moveOrgNode(payload: MoveOrgNodePayload): Promise<OrgNode> {
  try {
    const raw = await invoke<unknown>("move_org_node", { payload });
    return OrgNodeSchema.parse(raw) as OrgNode;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

export async function deactivateOrgNode(
  nodeId: number,
  expectedRowVersion: number,
): Promise<OrgNode> {
  try {
    const raw = await invoke<unknown>("deactivate_org_node", { nodeId, expectedRowVersion });
    return OrgNodeSchema.parse(raw) as OrgNode;
  } catch (err) {
    rethrowIfVersionConflict(err);
  }
}

// â”€â”€ Responsibility commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listOrgNodeResponsibilities(
  nodeId: number,
  includeInactive = false,
): Promise<OrgNodeResponsibility[]> {
  const raw = await invoke<unknown>("list_org_node_responsibilities", { nodeId, includeInactive });
  return z.array(OrgNodeResponsibilitySchema).parse(raw) as OrgNodeResponsibility[];
}

export async function assignOrgNodeResponsibility(
  payload: AssignResponsibilityPayload,
): Promise<OrgNodeResponsibility> {
  const raw = await invoke<unknown>("assign_org_node_responsibility", { payload });
  return OrgNodeResponsibilitySchema.parse(raw) as OrgNodeResponsibility;
}

export async function endOrgNodeResponsibility(
  assignmentId: number,
  validTo?: string,
): Promise<OrgNodeResponsibility> {
  const raw = await invoke<unknown>("end_org_node_responsibility", {
    assignmentId,
    validTo: validTo ?? null,
  });
  return OrgNodeResponsibilitySchema.parse(raw) as OrgNodeResponsibility;
}

// â”€â”€ Entity binding commands â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function listOrgEntityBindings(
  nodeId: number,
  includeInactive = false,
): Promise<OrgEntityBinding[]> {
  const raw = await invoke<unknown>("list_org_entity_bindings", { nodeId, includeInactive });
  return z.array(OrgEntityBindingSchema).parse(raw) as OrgEntityBinding[];
}

export async function upsertOrgEntityBinding(
  payload: UpsertOrgEntityBindingPayload,
): Promise<OrgEntityBinding> {
  const raw = await invoke<unknown>("upsert_org_entity_binding", { payload });
  return OrgEntityBindingSchema.parse(raw) as OrgEntityBinding;
}

export async function expireOrgEntityBinding(
  bindingId: number,
  validTo?: string,
): Promise<OrgEntityBinding> {
  const raw = await invoke<unknown>("expire_org_entity_binding", {
    bindingId,
    validTo: validTo ?? null,
  });
  return OrgEntityBindingSchema.parse(raw) as OrgEntityBinding;
}

// â”€â”€ Equipment assignment commands (GAP ORG-02) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export function listOrgNodeEquipment(nodeId: number): Promise<OrgNodeEquipmentRow[]> {
  return invoke<OrgNodeEquipmentRow[]>("list_org_node_equipment", { nodeId });
}

export function searchUnassignedEquipment(
  query: string,
  limit?: number,
): Promise<OrgNodeEquipmentRow[]> {
  return invoke<OrgNodeEquipmentRow[]>("search_unassigned_equipment", {
    query,
    limit: limit ?? 20,
  });
}

export function assignEquipmentToNode(payload: AssignEquipmentPayload): Promise<void> {
  return invoke<void>("assign_equipment_to_node", { payload });
}

export function unassignEquipmentFromNode(equipmentId: number): Promise<void> {
  return invoke<void>("unassign_equipment_from_node", { equipmentId });
}
