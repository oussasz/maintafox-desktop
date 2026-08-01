/**
 * Resolve allowed org node types for create-dialog UI.
 *
 * Types and rules MUST come from the same structure model as the parent node.
 * Prefer `parentTypeId` when the parent row is already in that model (draft tree).
 * Fall back to `parentTypeCode` only when bridging legacy mismatches.
 */

import type { OrgNodeType, OrgRelationshipRule } from "@shared/ipc-types";

export type CreateNodeMode = "root" | "child";

export function resolveAllowedNodeTypes(args: {
  mode: CreateNodeMode;
  types: OrgNodeType[];
  rules: OrgRelationshipRule[];
  parentTypeCode?: string | null;
  /** Same-model parent type id (preferred). */
  parentTypeId?: number | null;
}): OrgNodeType[] {
  const { mode, types, rules, parentTypeCode, parentTypeId } = args;
  const activeTypes = types.filter((t) => t.is_active);

  if (mode === "root") {
    return activeTypes.filter((t) => t.is_root_type);
  }

  const parentTypeInModel =
    parentTypeId != null
      ? activeTypes.find((t) => t.id === parentTypeId)
      : parentTypeCode
        ? activeTypes.find((t) => t.code === parentTypeCode)
        : undefined;

  if (!parentTypeInModel) return [];

  const childIds = new Set(
    rules
      .filter((r) => r.parent_type_id === parentTypeInModel.id)
      .map((r) => r.child_type_id),
  );

  return activeTypes.filter((t) => childIds.has(t.id) && !t.is_root_type);
}
