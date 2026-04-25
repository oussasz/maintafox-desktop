/**
 * org-service.ts
 *
 * IPC wrappers for the Organization module commands.
 * RULE: All invoke() calls for org commands are isolated here.
 *
 * This file covers the configuration layer (structure models, node types,
 * relationship rules). Node CRUD and responsibility bindings are in
 * org-node-service.ts (SP01-F02).
 */

import { invoke } from "@/lib/ipc-invoke";
import type {
  CreateOrgNodeTypePayload,
  CreateRelationshipRulePayload,
  CreateStructureModelPayload,
  OrgNodeType,
  OrgRelationshipRule,
  OrgStructureModel,
  UpdateOrgNodeTypePayload,
} from "@shared/ipc-types";

// â”€â”€â”€ Structure models â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export function listOrgStructureModels(): Promise<OrgStructureModel[]> {
  return invoke<OrgStructureModel[]>("list_org_structure_models");
}

export function getActiveOrgStructureModel(): Promise<OrgStructureModel | null> {
  return invoke<OrgStructureModel | null>("get_active_org_structure_model");
}

export function createOrgStructureModel(description?: string | null): Promise<OrgStructureModel> {
  return invoke<OrgStructureModel>("create_org_structure_model", {
    payload: { description: description ?? null } satisfies CreateStructureModelPayload,
  });
}

/** New draft: copy published node types and rules from the active model (org.admin). */
export function forkOrgDraftFromPublished(description?: string | null): Promise<OrgStructureModel> {
  return invoke<OrgStructureModel>("fork_org_draft_from_published", {
    payload: { description: description ?? null } satisfies CreateStructureModelPayload,
  });
}

export function publishOrgStructureModel(modelId: number): Promise<OrgStructureModel> {
  return invoke<OrgStructureModel>("publish_org_structure_model", {
    modelId,
  });
}

export function archiveOrgStructureModel(modelId: number): Promise<OrgStructureModel> {
  return invoke<OrgStructureModel>("archive_org_structure_model", {
    modelId,
  });
}

export function listOrgNodeTypes(structureModelId: number): Promise<OrgNodeType[]> {
  return invoke<OrgNodeType[]>("list_org_node_types", {
    structureModelId,
  });
}

export function createOrgNodeType(payload: CreateOrgNodeTypePayload): Promise<OrgNodeType> {
  return invoke<OrgNodeType>("create_org_node_type", { payload });
}

export function deactivateOrgNodeType(nodeTypeId: number): Promise<OrgNodeType> {
  return invoke<OrgNodeType>("deactivate_org_node_type", { nodeTypeId });
}

export function updateOrgNodeType(payload: UpdateOrgNodeTypePayload): Promise<OrgNodeType> {
  return invoke<OrgNodeType>("update_org_node_type", { payload });
}

export function getOrgNodeTypeUsageCount(nodeTypeId: number): Promise<number> {
  return invoke<number>("get_org_node_type_usage_count", { nodeTypeId });
}

// â”€â”€â”€ Relationship rules â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

export function listOrgRelationshipRules(structureModelId: number): Promise<OrgRelationshipRule[]> {
  return invoke<OrgRelationshipRule[]>("list_org_relationship_rules", {
    structureModelId,
  });
}

export function createOrgRelationshipRule(
  payload: CreateRelationshipRulePayload,
): Promise<OrgRelationshipRule> {
  return invoke<OrgRelationshipRule>("create_org_relationship_rule", {
    payload,
  });
}

export function deleteOrgRelationshipRule(ruleId: number): Promise<void> {
  return invoke<void>("delete_org_relationship_rule", { ruleId });
}
