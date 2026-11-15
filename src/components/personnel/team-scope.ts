/**
 * Which teams a person can belong to, given the unit (site, workshop…) they are attached to:
 * the teams of that unit, of the units below it, and of the units above it (a workshop's person
 * may belong to a team of its parent site). The server filter only covers the unit and what is
 * below it, so a workshop used to show no team at all.
 */
import type { OrgDesignerNodeRow, TeamSummary } from "@shared/ipc-types";

export function teamsForEntity(
  teams: TeamSummary[],
  nodes: Pick<OrgDesignerNodeRow, "node_id" | "ancestor_path">[],
  entityId: number,
): TeamSummary[] {
  const entity = nodes.find((n) => n.node_id === entityId);
  if (!entity) return teams.filter((t) => t.primary_node_id === entityId);
  const above = new Set(
    entity.ancestor_path
      .split("/")
      .filter(Boolean)
      .map(Number)
      .filter((id) => Number.isFinite(id)),
  ); // includes the unit itself
  const below = new Set(
    nodes.filter((n) => n.ancestor_path.startsWith(entity.ancestor_path)).map((n) => n.node_id),
  );
  return teams.filter(
    (t) =>
      t.primary_node_id != null && (above.has(t.primary_node_id) || below.has(t.primary_node_id)),
  );
}
