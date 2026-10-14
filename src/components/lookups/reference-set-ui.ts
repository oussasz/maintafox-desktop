/**
 * Presentation of reference set versions and of the synthetic (live table) domains.
 * No business rule here: what a user may do comes from the governance capabilities (IPC).
 */

import type { Tone } from "@/design-system/tone";
import type { ReferenceSet } from "@shared/ipc-types";

/** Lifecycle of a versioned set → tone (same vocabulary as every other module). */
export const REFERENCE_SET_STATUS_TONE: Record<string, Tone> = {
  draft: "progress",
  validated: "info",
  published: "success",
  superseded: "neutral",
};

const SET_STATUS_LABEL_KEY: Record<string, string> = {
  draft: "browser.status.draft",
  validated: "browser.status.validated",
  published: "browser.status.published",
  superseded: "browser.status.superseded",
};

/** i18n key (namespace `reference`) of a set status; unknown statuses read as "unknown status". */
export function referenceSetStatusLabelKey(status: string): string {
  return SET_STATUS_LABEL_KEY[status] ?? "browser.status.unknown";
}

/**
 * Synthetic domains are live tables of other modules (work-order types, inventory families, …)
 * shown in the Reference data tree. They have no versions: negative ids mark them.
 */
export function isSyntheticReferenceId(id: number | null | undefined): boolean {
  return id != null && id < 0;
}

/** Category B domains are one working catalogue: versions are not shown to users. */
export function isWorkingCatalogCategory(category: string | null | undefined): boolean {
  return category === "operational_dictionary";
}

/** The draft (or validated candidate) of a domain, if one is in progress. */
export function pendingCandidateSet(sets: readonly ReferenceSet[]): ReferenceSet | null {
  return (
    [...sets]
      .filter((s) => s.status === "draft" || s.status === "validated")
      .sort((a, b) => b.version_no - a.version_no)[0] ?? null
  );
}

const SYNTHETIC_DOMAIN_NAME_KEY: Record<string, string> = {
  "INVENTORY.ARTICLE_FAMILY": "synthetic.domains.articleFamilies",
  "INVENTORY.TAX_CATEGORY": "synthetic.domains.taxCategories",
  "WO.WORK_ORDER_TYPES": "synthetic.domains.woTypes",
  "WO.WORK_ORDER_PRIORITIES": "synthetic.domains.woPriorities",
  "WO.WORK_ORDER_STATUSES": "synthetic.domains.woStatuses",
};

/** Name shown for a domain: translated for the synthetic live tables, stored name otherwise. */
export function referenceDomainLabel(
  domain: { id: number; code: string; name: string },
  t: (key: string) => string,
): string {
  if (isSyntheticReferenceId(domain.id)) {
    const key = SYNTHETIC_DOMAIN_NAME_KEY[domain.code];
    if (key) return t(key);
  }
  return domain.name;
}
