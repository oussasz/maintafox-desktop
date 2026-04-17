/**
 * reference-manager-store.ts
 *
 * UI state for the Reference Manager page: domain catalog browsing,
 * set hierarchy, selection tracking, and client-side search filtering.
 *
 * Phase 2 – Sub-phase 03 – Sprint S4 (GAP REF-01).
 */

import { create } from "zustand";

import { listReferenceDomains, listReferenceSets } from "@/services/reference-service";
import { toErrorMessage } from "@/utils/errors";
import type { ReferenceDomain, ReferenceSet } from "@shared/ipc-types";

export const INVENTORY_ARTICLE_FAMILY_DOMAIN_ID = -101;
export const INVENTORY_ARTICLE_FAMILY_SET_ID = -101;
export const INVENTORY_TAX_CATEGORY_DOMAIN_ID = -102;
export const INVENTORY_TAX_CATEGORY_SET_ID = -102;

const INVENTORY_ARTICLE_FAMILY_DOMAIN: ReferenceDomain = {
  id: INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  code: "INVENTORY.ARTICLE_FAMILY",
  name: "Familles articles de stock",
  structure_type: "flat",
  governance_level: "tenant_managed",
  is_extendable: true,
  validation_rules_json: null,
  created_at: "",
  updated_at: "",
};

const INVENTORY_ARTICLE_FAMILY_SET: ReferenceSet = {
  id: INVENTORY_ARTICLE_FAMILY_SET_ID,
  domain_id: INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  version_no: 1,
  status: "published",
  effective_from: null,
  created_by_id: null,
  created_at: "",
  published_at: null,
};

const INVENTORY_TAX_CATEGORY_DOMAIN: ReferenceDomain = {
  id: INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  code: "INVENTORY.TAX_CATEGORY",
  name: "Catégories TVA articles de stock",
  structure_type: "flat",
  governance_level: "tenant_managed",
  is_extendable: true,
  validation_rules_json: null,
  created_at: "",
  updated_at: "",
};

const INVENTORY_TAX_CATEGORY_SET: ReferenceSet = {
  id: INVENTORY_TAX_CATEGORY_SET_ID,
  domain_id: INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  version_no: 1,
  status: "published",
  effective_from: null,
  created_by_id: null,
  created_at: "",
  published_at: null,
};

// ── Store interface ───────────────────────────────────────────────────────────

interface ReferenceManagerStoreState {
  /** All reference domains loaded from backend */
  domains: ReferenceDomain[];
  domainsLoading: boolean;

  /** Sets keyed by domain_id — loaded lazily on expand */
  setsMap: Record<number, ReferenceSet[]>;
  setsLoading: Record<number, boolean>;

  /** Currently selected domain */
  selectedDomainId: number | null;
  /** Currently selected set (drives right-pane content) */
  selectedSetId: number | null;

  /** Domain IDs currently expanded in the tree */
  expandedDomainIds: number[];

  /** Client-side filter text for the domain browser */
  searchFilter: string;

  error: string | null;

  // ── Actions ──────────────────────────────────────────────────────────────

  /** Load all domains from backend */
  loadDomains: () => Promise<void>;
  /** Load sets for a specific domain */
  loadSetsForDomain: (domainId: number) => Promise<void>;
  /** Select a domain (and expand it) */
  selectDomain: (domainId: number) => void;
  /** Select a set (and ensure its parent domain is selected) */
  selectSet: (setId: number, domainId: number) => void;
  /** Toggle domain expanded/collapsed */
  toggleDomainExpanded: (domainId: number) => void;
  /** Update client-side search filter */
  setSearchFilter: (filter: string) => void;
  /** Clear selection */
  clearSelection: () => void;
  clearError: () => void;
}

// ── Store implementation ──────────────────────────────────────────────────────

export const useReferenceManagerStore = create<ReferenceManagerStoreState>()((set, get) => ({
  domains: [],
  domainsLoading: false,
  setsMap: {},
  setsLoading: {},
  selectedDomainId: null,
  selectedSetId: null,
  expandedDomainIds: [],
  searchFilter: "",
  error: null,

  loadDomains: async () => {
    set({ domainsLoading: true, error: null });
    try {
      const domains = await listReferenceDomains();
      const hasInventoryFamilyDomain = domains.some(
        (domain) => domain.id === INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
      );
      const hasInventoryTaxDomain = domains.some((domain) => domain.id === INVENTORY_TAX_CATEGORY_DOMAIN_ID);
      const nextDomains = [
        ...domains,
        ...(hasInventoryFamilyDomain ? [] : [INVENTORY_ARTICLE_FAMILY_DOMAIN]),
        ...(hasInventoryTaxDomain ? [] : [INVENTORY_TAX_CATEGORY_DOMAIN]),
      ].sort((a, b) => a.name.localeCompare(b.name));
      set({
        domains: nextDomains,
        setsMap: {
          ...get().setsMap,
          [INVENTORY_ARTICLE_FAMILY_DOMAIN_ID]: [INVENTORY_ARTICLE_FAMILY_SET],
          [INVENTORY_TAX_CATEGORY_DOMAIN_ID]: [INVENTORY_TAX_CATEGORY_SET],
        },
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({ domainsLoading: false });
    }
  },

  loadSetsForDomain: async (domainId) => {
    if (domainId === INVENTORY_ARTICLE_FAMILY_DOMAIN_ID) {
      set({
        setsMap: {
          ...get().setsMap,
          [INVENTORY_ARTICLE_FAMILY_DOMAIN_ID]: [INVENTORY_ARTICLE_FAMILY_SET],
        },
      });
      return;
    }
    if (domainId === INVENTORY_TAX_CATEGORY_DOMAIN_ID) {
      set({
        setsMap: {
          ...get().setsMap,
          [INVENTORY_TAX_CATEGORY_DOMAIN_ID]: [INVENTORY_TAX_CATEGORY_SET],
        },
      });
      return;
    }

    const { setsLoading } = get();
    if (setsLoading[domainId]) return;

    set({
      setsLoading: { ...get().setsLoading, [domainId]: true },
      error: null,
    });
    try {
      const sets = await listReferenceSets(domainId);
      set({
        setsMap: { ...get().setsMap, [domainId]: sets },
      });
    } catch (err) {
      set({ error: toErrorMessage(err) });
    } finally {
      set({
        setsLoading: { ...get().setsLoading, [domainId]: false },
      });
    }
  },

  selectDomain: (domainId) => {
    const { expandedDomainIds } = get();
    const isExpanded = expandedDomainIds.includes(domainId);
    set({
      selectedDomainId: domainId,
      selectedSetId: null,
      expandedDomainIds: isExpanded ? expandedDomainIds : [...expandedDomainIds, domainId],
    });
    // Eagerly load sets when domain is selected
    if (!get().setsMap[domainId]) {
      void get().loadSetsForDomain(domainId);
    }
  },

  selectSet: (setId, domainId) => {
    const { expandedDomainIds } = get();
    set({
      selectedDomainId: domainId,
      selectedSetId: setId,
      expandedDomainIds: expandedDomainIds.includes(domainId)
        ? expandedDomainIds
        : [...expandedDomainIds, domainId],
    });
  },

  toggleDomainExpanded: (domainId) => {
    const { expandedDomainIds } = get();
    const isExpanded = expandedDomainIds.includes(domainId);
    set({
      expandedDomainIds: isExpanded
        ? expandedDomainIds.filter((id) => id !== domainId)
        : [...expandedDomainIds, domainId],
    });
    // Load sets on first expand
    if (!isExpanded && !get().setsMap[domainId]) {
      void get().loadSetsForDomain(domainId);
    }
  },

  setSearchFilter: (filter) => set({ searchFilter: filter }),

  clearSelection: () => set({ selectedDomainId: null, selectedSetId: null }),

  clearError: () => set({ error: null }),
}));
