// Merge rule of scripts/sync-allowlist-append.mjs, kept apart so it can be tested.
//
// The control-plane allowlist is append-only (maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md
// §2): existing entries are never changed or removed. Each entry carries `append_only`, true
// exactly for the desktop's conflict_policy "append_only" (the server refuses deletes of those
// types and refuses to start when an entry lacks the flag).

/** True when the desktop stages this entity type to the server. */
const isStaged = (entity) =>
  entity.sync_status === "synced" || entity.sync_status === "staged_only";

/**
 * Appends to `server.entity_types` (in place) every staged desktop type it does not list yet.
 * Returns `{ added, problems }`. `problems` lists entries already in the allowlist whose scope or
 * `append_only` flag disagrees with the desktop registry; the caller must not write the file then,
 * because an existing entry is never rewritten.
 */
export function appendStagedTypes(registry, server) {
  const listed = new Map(server.entity_types.map((e) => [e.entity_type, e]));
  const added = [];
  const problems = [];
  for (const entity of registry.entities) {
    if (!isStaged(entity)) continue;
    const appendOnly = entity.conflict_policy === "append_only";
    const entry = listed.get(entity.entity_type);
    if (entry === undefined) {
      const created = {
        entity_type: entity.entity_type,
        scope: entity.scope,
        append_only: appendOnly,
      };
      server.entity_types.push(created);
      listed.set(entity.entity_type, created);
      added.push(`${entity.entity_type} (${entity.scope}${appendOnly ? ", append-only" : ""})`);
      continue;
    }
    if (entry.scope !== entity.scope) {
      problems.push(
        `${entity.entity_type}: allowlist scope ${entry.scope}, desktop scope ${entity.scope}`,
      );
    }
    if (typeof entry.append_only !== "boolean") {
      problems.push(
        `${entity.entity_type}: allowlist entry has no append_only flag (desktop: ${appendOnly})`,
      );
    } else if (entry.append_only !== appendOnly) {
      problems.push(
        `${entity.entity_type}: allowlist append_only ${entry.append_only}, desktop conflict_policy ${entity.conflict_policy}`,
      );
    }
  }
  return { added, problems };
}
