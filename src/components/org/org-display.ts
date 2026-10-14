/**
 * org-display.ts
 *
 * Display helpers shared by the organization designer: translated unit status, the path of a
 * unit from the top of the structure, responsibility type labels, and the units a unit may be
 * moved under. Pure functions — no IPC.
 */

import type { TFunction } from "i18next";

import { describeAppError } from "@/lib/app-error";
import type {
  LookupValueOption,
  OrgDesignerNodeRow,
  OrgDesignerSnapshot,
  OrgRelationshipRule,
} from "@shared/ipc-types";

export function orgNodeStatusLabel(t: TFunction<"org">, status: string): string {
  switch (status) {
    case "active":
      return t("designer.statusActive");
    case "inactive":
      return t("designer.statusInactive");
    case "draft":
      return t("designer.statusDraft");
    default:
      return t("designer.statusOther");
  }
}

/** Units from the top of the structure down to `nodeId` (inclusive). */
export function orgNodePath(
  snapshot: OrgDesignerSnapshot | null,
  nodeId: number,
): OrgDesignerNodeRow[] {
  if (!snapshot) return [];
  const byId = new Map(snapshot.nodes.map((n) => [n.node_id, n]));
  const path: OrgDesignerNodeRow[] = [];
  const seen = new Set<number>();
  let current = byId.get(nodeId);
  while (current && !seen.has(current.node_id)) {
    seen.add(current.node_id);
    path.unshift(current);
    current = current.parent_id != null ? byId.get(current.parent_id) : undefined;
  }
  return path;
}

export function orgNodePathLabel(snapshot: OrgDesignerSnapshot | null, nodeId: number): string {
  return orgNodePath(snapshot, nodeId)
    .map((n) => n.name)
    .join(" › ");
}

/**
 * Units `node` can be moved under: same structure version, active, not the unit itself or one of
 * its sub-units, not its current parent, and allowed by the parent → child rules of the model.
 * The backend preview re-checks all of this before anything is applied.
 */
export function moveParentCandidates(
  snapshot: OrgDesignerSnapshot | null,
  node: OrgDesignerNodeRow,
  rules: OrgRelationshipRule[],
): OrgDesignerNodeRow[] {
  if (!snapshot) return [];
  const allowedParentTypes = new Set(
    rules.filter((r) => r.child_type_id === node.node_type_id).map((r) => r.parent_type_id),
  );
  const ownSegment = `/${node.node_id}/`;
  return snapshot.nodes.filter(
    (n) =>
      n.node_id !== node.node_id &&
      n.node_id !== node.parent_id &&
      n.status === "active" &&
      !`${n.ancestor_path}/`.includes(ownSegment) &&
      (node.structure_model_id == null || n.structure_model_id === node.structure_model_id) &&
      allowedParentTypes.has(n.node_type_id),
  );
}

/** Label of a governed lookup value in the app language (FR/EN), else its default label. */
export function lookupLabel(value: LookupValueOption, language: string): string {
  if (language.startsWith("fr") && value.fr_label) return value.fr_label;
  if (language.startsWith("en") && value.en_label) return value.en_label;
  return value.label || value.code;
}

/** Responsibility type label: lookup codes match case-insensitively (legacy rows are lower case). */
export function responsibilityTypeLabel(
  code: string,
  types: LookupValueOption[],
  language: string,
): string {
  const match = types.find((v) => v.code.toLowerCase() === code.toLowerCase());
  return match ? lookupLabel(match, language) : code;
}

/**
 * One-line, translated text for a failed org call shown next to the field it concerns.
 * Org validation failures carry localized issue text; anything else uses the shared
 * translated title (never the raw backend message or `String(err)`).
 */
export function orgErrorText(err: unknown): string {
  const described = describeAppError(err);
  if (described.code === "VALIDATION_FAILED" && described.description) return described.description;
  return described.title;
}
