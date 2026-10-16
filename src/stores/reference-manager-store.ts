/**
 * reference-manager-store.ts
 *
 * UI state for the Reference Manager page: domain catalog browsing,
 * set hierarchy, selection tracking, and client-side search filtering.
 *
 * Phase 2 – Sub-phase 03 – Sprint S4 (GAP REF-01).
 */

import { create } from "zustand";

import { preferredWorkingSet } from "@/lib/reference-governance-ui";
import {
  listReferenceDomains,
  listReferenceModuleCatalogs,
  listReferenceSets,
  getReferenceGovernanceCapabilities,
} from "@/services/reference-service";
import { referenceErrorText } from "@/stores/reference-error-text";
import type {
  ReferenceDomain,
  ReferenceGovernanceCapabilities,
  ReferenceSet,
} from "@shared/ipc-types";

/**
 * Domains seeded in demo/legacy data that are superseded by a synthetic sidebar entry.
 * `WORK.PRIORITY` duplicates the urgency scale (synthetic -104, `WORK.URGENCY`).
 */
const SUPERSEDED_REFERENCE_DOMAIN_CODES = new Set<string>(["WORK.PRIORITY"]);

export const INVENTORY_ARTICLE_FAMILY_DOMAIN_ID = -101;
export const INVENTORY_ARTICLE_FAMILY_SET_ID = -101;
export const INVENTORY_TAX_CATEGORY_DOMAIN_ID = -102;
export const INVENTORY_TAX_CATEGORY_SET_ID = -102;
/** Company attributes (labels, target times) of the shared DI / OT urgency scale. */
export const URGENCY_SCALE_DOMAIN_ID = -104;
export const URGENCY_SCALE_SET_ID = -104;
export const WORK_ORDER_STATUSES_DOMAIN_ID = -105;
export const WORK_ORDER_STATUSES_SET_ID = -105;
/** The plant's notification settings: recipients, acknowledgement, escalation, retention. */
export const NOTIFICATION_RULES_DOMAIN_ID = -106;
/** Notification categories (SYSTEM: listed for the audience map only, never shown). */
export const NOTIFICATION_CATEGORIES_DOMAIN_ID = -107;

/**
 * Module tables shown in the tree next to the reference domains (no versions: one live set).
 * Only the wiring lives here (id, code, name, legacy governance fields). Whether a catalog is a
 * Maintafox list (hidden) or a company list (shown) comes from the backend
 * (`list_reference_module_catalogs`), like the audience of the real domains.
 */
interface SyntheticCatalog {
  domain: Omit<ReferenceDomain, "audience">;
  set: ReferenceSet;
}

function syntheticCatalog(
  id: number,
  code: string,
  name: string,
  governance: Pick<ReferenceDomain, "governance_level" | "governance_category" | "is_extendable">,
): SyntheticCatalog {
  return {
    domain: {
      id,
      code,
      name,
      structure_type: "flat",
      ...governance,
      validation_rules_json: null,
      created_at: "",
      updated_at: "",
    },
    set: {
      id,
      domain_id: id,
      version_no: 1,
      status: "published",
      effective_from: null,
      created_by_id: null,
      created_at: "",
      published_at: null,
    },
  };
}

const TENANT_DICTIONARY = {
  governance_level: "tenant_managed",
  governance_category: "operational_dictionary",
  is_extendable: true,
} as const;

const SYNTHETIC_CATALOGS: readonly SyntheticCatalog[] = [
  syntheticCatalog(
    INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
    "INVENTORY.ARTICLE_FAMILY",
    "Familles articles de stock",
    TENANT_DICTIONARY,
  ),
  syntheticCatalog(
    INVENTORY_TAX_CATEGORY_DOMAIN_ID,
    "INVENTORY.TAX_CATEGORY",
    "Catégories TVA articles de stock",
    TENANT_DICTIONARY,
  ),
  syntheticCatalog(URGENCY_SCALE_DOMAIN_ID, "WORK.URGENCY_SCALE", "Échelle d'urgence (DI / OT)", {
    governance_level: "protected_analytical",
    governance_category: "system_catalog",
    is_extendable: false,
  }),
  syntheticCatalog(
    WORK_ORDER_STATUSES_DOMAIN_ID,
    "WO.WORK_ORDER_STATUSES",
    "Statuts d'ordre de travail",
    {
      governance_level: "protected_analytical",
      governance_category: "system_catalog",
      is_extendable: false,
    },
  ),
  syntheticCatalog(
    NOTIFICATION_RULES_DOMAIN_ID,
    "NOTIFICATION.RULES",
    "Notifications (destinataires et escalade)",
    TENANT_DICTIONARY,
  ),
  syntheticCatalog(
    NOTIFICATION_CATEGORIES_DOMAIN_ID,
    "NOTIFICATION.CATEGORIES",
    "Catégories de notification",
    {
      governance_level: "protected_analytical",
      governance_category: "system_catalog",
      is_extendable: false,
    },
  ),
];

function syntheticCatalogById(domainId: number): SyntheticCatalog | undefined {
  return SYNTHETIC_CATALOGS.find((c) => c.domain.id === domainId);
}

/**
 * Données de référence lists only the company's own lists. Maintafox lists (`audience: system`)
 * stay in the engine for pickers and links but are not managed here.
 */
function isCompanyList(domain: ReferenceDomain): boolean {
  return domain.audience === "company";
}

// ── Store interface ───────────────────────────────────────────────────────────

interface ReferenceManagerStoreState {
  /** All reference domains loaded from backend */
  domains: ReferenceDomain[];
  domainsLoading: boolean;

  /** Domain-level capabilities (no set) from governance engine */
  domainCapabilities: Record<number, ReferenceGovernanceCapabilities>;

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
  domainCapabilities: {},
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
      const [domains, moduleCatalogs] = await Promise.all([
        listReferenceDomains(),
        listReferenceModuleCatalogs(),
      ]);
      const filteredDomains = domains.filter(
        (domain) =>
          isCompanyList(domain) &&
          !SUPERSEDED_REFERENCE_DOMAIN_CODES.has(domain.code.trim().toUpperCase()),
      );
      const backendIds = new Set(domains.map((domain) => domain.id));
      const audienceByCode = new Map(
        moduleCatalogs.map((c) => [c.code.trim().toUpperCase(), c.audience] as const),
      );
      // A module catalog the backend does not classify is not shown (no frontend default).
      const syntheticDomains: ReferenceDomain[] = [];
      const syntheticSets: Record<number, ReferenceSet[]> = {};
      for (const catalog of SYNTHETIC_CATALOGS) {
        const audience = audienceByCode.get(catalog.domain.code);
        if (audience == null || backendIds.has(catalog.domain.id)) continue;
        const domain: ReferenceDomain = { ...catalog.domain, audience };
        if (!isCompanyList(domain)) continue;
        syntheticDomains.push(domain);
        syntheticSets[domain.id] = [catalog.set];
      }
      const nextDomains = [...filteredDomains, ...syntheticDomains].sort((a, b) =>
        a.name.localeCompare(b.name),
      );

      const capabilityEntries = await Promise.all(
        filteredDomains.map(async (d) => {
          try {
            const caps = await getReferenceGovernanceCapabilities(d.id, null);
            return [d.id, caps] as const;
          } catch {
            return null;
          }
        }),
      );
      const domainCapabilities: Record<number, ReferenceGovernanceCapabilities> = {};
      for (const entry of capabilityEntries) {
        if (entry) domainCapabilities[entry[0]] = entry[1];
      }

      set({
        domains: nextDomains,
        domainCapabilities,
        setsMap: { ...get().setsMap, ...syntheticSets },
      });
    } catch (err) {
      set({ error: referenceErrorText(err) });
    } finally {
      set({ domainsLoading: false });
    }
  },

  loadSetsForDomain: async (domainId) => {
    const synthetic = syntheticCatalogById(domainId);
    if (synthetic) {
      set({ setsMap: { ...get().setsMap, [domainId]: [synthetic.set] } });
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
      // Category B: auto-open the published working catalog.
      const domain = get().domains.find((d) => d.id === domainId);
      if (
        domain?.governance_category === "operational_dictionary" &&
        get().selectedDomainId === domainId &&
        get().selectedSetId == null
      ) {
        const working = preferredWorkingSet(sets);
        if (working) {
          get().selectSet(working.id, domainId);
        }
      }
    } catch (err) {
      set({ error: referenceErrorText(err) });
    } finally {
      set({
        setsLoading: { ...get().setsLoading, [domainId]: false },
      });
    }
  },

  selectDomain: (domainId) => {
    const { expandedDomainIds, setsMap, domains } = get();
    const isExpanded = expandedDomainIds.includes(domainId);
    const domain = domains.find((d) => d.id === domainId);
    const existingSets = setsMap[domainId];
    let autoSetId: number | null = null;
    if (domain?.governance_category === "operational_dictionary" && existingSets) {
      autoSetId = preferredWorkingSet(existingSets)?.id ?? null;
    }
    set({
      selectedDomainId: domainId,
      selectedSetId: autoSetId,
      expandedDomainIds: isExpanded ? expandedDomainIds : [...expandedDomainIds, domainId],
    });
    // Eagerly load sets when domain is selected
    if (!existingSets) {
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
