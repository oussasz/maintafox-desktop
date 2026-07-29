/**
 * org-designer-service.ts
 *
 * IPC wrappers for the org designer workspace. Exposes snapshot projections
 * and impact-preview endpoints added in SP01-F03 S1.
 */

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { OrgValidationIssueSchema } from "@/services/org-governance-service";
import type {
  OrgDesignerNodeRow,
  OrgDesignerSnapshot,
  OrgImpactPreview,
  PreviewOrgChangePayload,
} from "@shared/ipc-types";

// â”€â”€ Zod schemas â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

const OrgDesignerNodeRowSchema = z.object({
  node_id: z.number(),
  parent_id: z.number().nullable(),
  ancestor_path: z.string(),
  depth: z.number(),
  code: z.string(),
  name: z.string(),
  status: z.string(),
  row_version: z.number(),
  node_type_id: z.number(),
  node_type_code: z.string(),
  node_type_label: z.string(),
  can_host_assets: z.boolean(),
  can_own_work: z.boolean(),
  can_carry_cost_center: z.boolean(),
  can_aggregate_kpis: z.boolean(),
  can_receive_permits: z.boolean(),
  child_count: z.number(),
  active_responsibility_count: z.number(),
  active_binding_count: z.number(),
  structure_model_id: z.number().optional(),
  origin_node_id: z.number().nullable().optional(),
});

const OrgDesignerSnapshotSchema = z.object({
  active_model_id: z.number().nullable(),
  active_model_version: z.number().nullable(),
  draft_model_id: z.number().nullable(),
  draft_model_version: z.number().nullable(),
  display_model_id: z.number().nullable().optional(),
  nodes: z.array(OrgDesignerNodeRowSchema),
});

const OrgImpactDependencySummarySchema = z.object({
  domain: z.string(),
  status: z.string(),
  count: z.number().nullable(),
  note: z.string().nullable(),
});

const OrgImpactPreviewSchema = z.object({
  action: z.enum(["MoveNode", "DeactivateNode", "ReassignResponsibility"]),
  subject_node_id: z.number(),
  affected_node_count: z.number(),
  descendant_count: z.number(),
  active_responsibility_count: z.number(),
  active_binding_count: z.number(),
  blockers: z.array(OrgValidationIssueSchema),
  warnings: z.array(OrgValidationIssueSchema),
  dependencies: z.array(OrgImpactDependencySummarySchema),
});

// â”€â”€ Service functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export async function getOrgDesignerSnapshot(preferDraft = false): Promise<OrgDesignerSnapshot> {
  const raw = await invoke<unknown>("get_org_designer_snapshot", {
    preferDraft,
  });
  return OrgDesignerSnapshotSchema.parse(raw) as OrgDesignerSnapshot;
}

export async function searchOrgDesignerNodes(
  query: string,
  statusFilter?: string | null,
  typeFilter?: string | null,
  modelId?: number | null,
): Promise<OrgDesignerNodeRow[]> {
  if (modelId == null) {
    throw new Error("searchOrgDesignerNodes requires modelId (display_model_id)");
  }
  const raw = await invoke<unknown>("search_org_designer_nodes", {
    query,
    statusFilter: statusFilter ?? null,
    typeFilter: typeFilter ?? null,
    modelId,
  });
  return z.array(OrgDesignerNodeRowSchema).parse(raw) as OrgDesignerNodeRow[];
}

export async function previewOrgChange(
  payload: PreviewOrgChangePayload,
): Promise<OrgImpactPreview> {
  const raw = await invoke<unknown>("preview_org_change", { payload });
  return OrgImpactPreviewSchema.parse(raw) as OrgImpactPreview;
}
