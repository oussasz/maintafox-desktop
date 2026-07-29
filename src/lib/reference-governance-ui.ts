/**
 * Presentation helpers for Reference Manager — no business-rule decisions.
 * Action enablement comes exclusively from ReferenceGovernanceCapabilities (IPC).
 */

import type { ReferenceSet } from "@shared/ipc-types";

export type GovernanceCategoryId =
  | "system_catalog"
  | "operational_dictionary"
  | "controlled_catalog";

export function isGovernanceCategory(raw: string | null | undefined): raw is GovernanceCategoryId {
  return (
    raw === "system_catalog" ||
    raw === "operational_dictionary" ||
    raw === "controlled_catalog"
  );
}

/** i18n key under `reference.governance.category.*` */
export function governanceCategoryLabelKey(category: string): string {
  if (category === "system_catalog") return "governance.category.systemCatalog";
  if (category === "operational_dictionary") return "governance.category.operationalDictionary";
  if (category === "controlled_catalog") return "governance.category.controlledCatalog";
  return "governance.category.unknown";
}

/**
 * Compact tree-badge label (sidebar). Full names stay in `governanceCategoryLabelKey`.
 * Keeps badge width predictable so domain titles are not truncated to a few letters.
 */
export function governanceCategoryShortLabelKey(category: string): string {
  if (category === "system_catalog") return "governance.categoryShort.systemCatalog";
  if (category === "operational_dictionary") {
    return "governance.categoryShort.operationalDictionary";
  }
  if (category === "controlled_catalog") return "governance.categoryShort.controlledCatalog";
  return "governance.categoryShort.unknown";
}

export type PublishedReadOnlyBannerKey =
  | "governance.readOnly.published"
  | "governance.readOnly.publishedProtected"
  | "governance.readOnly.superseded"
  | "governance.readOnly.supersededProtected";

/**
 * i18n key under `reference.governance.readOnly.*` for published/superseded banners.
 * `canCreateDraftSet` must already include both governance `can_create_draft_set`
 * and the actor's `ref.manage` permission — never suggest a draft the user cannot create.
 */
export function publishedReadOnlyBannerKey(
  setStatus: string,
  canCreateDraftSet: boolean,
): PublishedReadOnlyBannerKey {
  if (setStatus === "superseded") {
    return canCreateDraftSet
      ? "governance.readOnly.superseded"
      : "governance.readOnly.supersededProtected";
  }
  return canCreateDraftSet
    ? "governance.readOnly.published"
    : "governance.readOnly.publishedProtected";
}

export function showCategoryLockIcon(category: string): boolean {
  return category === "system_catalog";
}

/**
 * Sets shown in the Manager tree. Category B is a single working catalog —
 * only the published set is visible (historical draft/superseded hidden).
 */
export function visibleSetsForDomain(
  category: string | null | undefined,
  sets: ReferenceSet[],
): ReferenceSet[] {
  if (category === "operational_dictionary") {
    return sets.filter((s) => s.status === "published");
  }
  return sets;
}

/** Prefer the latest published set (highest version_no). */
export function preferredWorkingSet(sets: ReferenceSet[]): ReferenceSet | null {
  const published = sets
    .filter((s) => s.status === "published")
    .sort((a, b) => b.version_no - a.version_no);
  return published[0] ?? null;
}
