// Merge rule of scripts/sync-allowlist-append.mjs, kept apart so it can be tested.
//
// The control-plane allowlist is append-only (maintafox-vendor-console/deploy/SYNC_PROTOCOL_V2.md
// §2): an entry's type, scope and `append_only` flag are never changed or removed. Each entry
// carries `append_only`, true exactly for the desktop's conflict_policy "append_only" (the server
// refuses deletes of those types and refuses to start when an entry lacks the flag).
//
// Per-user signing (sync-signing-design.md v9 §5): the signing contract of an entry —
// `author_fields` ([{field, guards}]) and `authority` (the name of a plant-wide right) — is
// generated from the desktop row rules (src-tauri/src/sync/signing_rules.rs) and always copied from the
// desktop registry, on new and existing entries alike, so the server never judges authors or
// authority on a stale list. Types the server publishes but devices never push
// (`server_only_types` of the desktop registry, e.g. `user_signing_keys`) are listed with
// `server_only: true`; the desktop never stages them.

/** True when the desktop stages this entity type to the server. */
const isStaged = (entity) =>
  entity.sync_status === "synced" || entity.sync_status === "staged_only";

/** The desktop's signing contract for one entity, normalised (keys absent when not set). */
function desktopSigning(entity) {
  const out = {};
  if (entity.authority !== undefined && entity.authority !== null) out.authority = entity.authority;
  if (Array.isArray(entity.author_fields) && entity.author_fields.length > 0) {
    out.author_fields = entity.author_fields.map((a) => ({
      field: a.field,
      guards: [...a.guards],
    }));
  }
  return out;
}

const SIGNING_KEYS = ["authority", "author_fields"];

/**
 * What replacing the server's signing contract of `entry` with the desktop's `signing` would
 * weaken: an authority removed or changed, an author field dropped, or a guard dropped from one.
 * Such a change is never applied silently (review F9): it is reported as a problem, so a stale or
 * wrong desktop list cannot loosen what the server judges.
 */
function signingWeakenings(entry, signing) {
  const out = [];
  const type = entry.entity_type;
  if (entry.authority !== undefined && entry.authority !== null && entry.authority !== signing.authority) {
    out.push(`${type}: allowlist authority ${entry.authority}, desktop ${signing.authority ?? "none"}`);
  }
  const desktopFields = new Map((signing.author_fields ?? []).map((a) => [a.field, a]));
  for (const author of entry.author_fields ?? []) {
    const mine = desktopFields.get(author.field);
    if (mine === undefined) {
      out.push(`${type}: author field ${author.field} is in the allowlist, not on the desktop`);
      continue;
    }
    for (const guard of author.guards ?? []) {
      if (!mine.guards.includes(guard)) {
        out.push(`${type}: guard ${guard} of ${author.field} is in the allowlist, not on the desktop`);
      }
    }
  }
  return out;
}

/** Replaces the signing keys of `entry` (in place) with `signing`; true when anything changed. */
function setSigning(entry, signing) {
  let changed = false;
  for (const key of SIGNING_KEYS) {
    if (JSON.stringify(entry[key]) === JSON.stringify(signing[key])) continue;
    changed = true;
    if (signing[key] === undefined) delete entry[key];
    else entry[key] = signing[key];
  }
  return changed;
}

/**
 * Brings `server.entity_types` (in place) in line with the desktop registry:
 * - appends every staged desktop type it does not list yet, with its signing contract;
 * - refreshes the signing contract (`author_fields`, `authority`) of listed staged types, only
 *   to make it stricter or equal: a contract the desktop would weaken is a problem;
 * - appends every desktop `server_only_types` entry with `server_only: true`.
 * Returns `{ added, updated, problems }`. `problems` lists entries whose scope, `append_only` or
 * `server_only` flag disagrees with the desktop registry, or whose signing contract the desktop
 * would weaken; nothing is changed then, because those parts of an entry are never rewritten
 * (or loosened) by a merge.
 */
export function appendStagedTypes(registry, server) {
  const listed = new Map(server.entity_types.map((e) => [e.entity_type, e]));
  const problems = [];
  const staged = registry.entities.filter(isStaged);
  const stagedTypes = new Set(staged.map((e) => e.entity_type));
  const serverOnly = registry.server_only_types ?? [];

  for (const entity of staged) {
    const entry = listed.get(entity.entity_type);
    if (entry === undefined) continue;
    const appendOnly = entity.conflict_policy === "append_only";
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
    if (entry.server_only === true) {
      problems.push(`${entity.entity_type}: server-only in the allowlist, staged by the desktop`);
    }
    problems.push(...signingWeakenings(entry, desktopSigning(entity)));
  }
  for (const so of serverOnly) {
    if (stagedTypes.has(so.entity_type)) {
      problems.push(`${so.entity_type}: server-only on the desktop, which also stages it`);
    }
    const entry = listed.get(so.entity_type);
    if (entry === undefined) continue;
    if (entry.scope !== so.scope) {
      problems.push(`${so.entity_type}: allowlist scope ${entry.scope}, desktop scope ${so.scope}`);
    }
    if (entry.server_only !== true) {
      problems.push(
        `${so.entity_type}: allowlist entry is not server-only, desktop server-only type`,
      );
    }
  }
  if (problems.length) return { added: [], updated: [], problems };

  const added = [];
  const updated = [];
  for (const entity of staged) {
    const signing = desktopSigning(entity);
    const entry = listed.get(entity.entity_type);
    if (entry === undefined) {
      const appendOnly = entity.conflict_policy === "append_only";
      const created = {
        entity_type: entity.entity_type,
        scope: entity.scope,
        append_only: appendOnly,
        ...signing,
      };
      server.entity_types.push(created);
      listed.set(entity.entity_type, created);
      added.push(`${entity.entity_type} (${entity.scope}${appendOnly ? ", append-only" : ""})`);
      continue;
    }
    if (setSigning(entry, signing)) updated.push(`${entity.entity_type} (signing contract)`);
  }
  for (const so of serverOnly) {
    if (listed.has(so.entity_type)) continue;
    const created = {
      entity_type: so.entity_type,
      scope: so.scope,
      append_only: false,
      server_only: true,
    };
    server.entity_types.push(created);
    listed.set(so.entity_type, created);
    added.push(`${so.entity_type} (${so.scope}, server-only)`);
  }
  return { added, updated, problems };
}
