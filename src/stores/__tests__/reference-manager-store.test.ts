/**
 * reference-manager-store — behaviour tests.
 *
 * Mocks only the `@/services/reference-service` boundary; the real
 * `preferredWorkingSet` helper drives auto-selection.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getReferenceGovernanceCapabilities,
  listReferenceDomains,
  listReferenceSets,
} from "@/services/reference-service";
import type {
  ReferenceDomain,
  ReferenceGovernanceCapabilities,
  ReferenceSet,
} from "@shared/ipc-types";

import {
  INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  INVENTORY_ARTICLE_FAMILY_SET_ID,
  INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  INVENTORY_TAX_CATEGORY_SET_ID,
  WORK_ORDER_PRIORITIES_DOMAIN_ID,
  WORK_ORDER_PRIORITIES_SET_ID,
  WORK_ORDER_STATUSES_DOMAIN_ID,
  WORK_ORDER_STATUSES_SET_ID,
  WORK_ORDER_TYPES_DOMAIN_ID,
  WORK_ORDER_TYPES_SET_ID,
  useReferenceManagerStore,
} from "../reference-manager-store";

vi.mock("@/services/reference-service", () => ({
  getReferenceGovernanceCapabilities: vi.fn(),
  listReferenceDomains: vi.fn(),
  listReferenceSets: vi.fn(),
}));

const mockCaps = vi.mocked(getReferenceGovernanceCapabilities);
const mockDomains = vi.mocked(listReferenceDomains);
const mockSets = vi.mocked(listReferenceSets);

const SYNTHETIC_DOMAIN_IDS = [
  INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  WORK_ORDER_TYPES_DOMAIN_ID,
  WORK_ORDER_PRIORITIES_DOMAIN_ID,
  WORK_ORDER_STATUSES_DOMAIN_ID,
];

const SYNTHETIC_PAIRS: Array<[number, number]> = [
  [INVENTORY_ARTICLE_FAMILY_DOMAIN_ID, INVENTORY_ARTICLE_FAMILY_SET_ID],
  [INVENTORY_TAX_CATEGORY_DOMAIN_ID, INVENTORY_TAX_CATEGORY_SET_ID],
  [WORK_ORDER_TYPES_DOMAIN_ID, WORK_ORDER_TYPES_SET_ID],
  [WORK_ORDER_PRIORITIES_DOMAIN_ID, WORK_ORDER_PRIORITIES_SET_ID],
  [WORK_ORDER_STATUSES_DOMAIN_ID, WORK_ORDER_STATUSES_SET_ID],
];

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeDomain(overrides: Partial<ReferenceDomain> = {}): ReferenceDomain {
  return {
    id: 1,
    code: "FAILURE.CAUSE",
    name: "Causes de défaillance",
    structure_type: "flat",
    governance_level: "tenant_managed",
    governance_category: "controlled_catalog",
    is_extendable: true,
    validation_rules_json: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function makeSet(overrides: Partial<ReferenceSet> = {}): ReferenceSet {
  return {
    id: 100,
    domain_id: 1,
    version_no: 1,
    status: "published",
    effective_from: null,
    created_by_id: 1,
    created_at: "2026-01-01T00:00:00Z",
    published_at: "2026-01-02T00:00:00Z",
    ...overrides,
  };
}

function makeCaps(
  overrides: Partial<ReferenceGovernanceCapabilities> = {},
): ReferenceGovernanceCapabilities {
  return {
    category: "controlled_catalog",
    enforcement_phase: "enforced",
    can_create_value: true,
    can_update_value: true,
    can_deactivate_value: true,
    can_create_draft_set: true,
    can_discard_draft_set: true,
    can_publish: true,
    can_operational_create: false,
    is_read_only: false,
    requires_analytical_protection: false,
    set_status: null,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  let reject: (reason: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const initialState = useReferenceManagerStore.getState();

function resetStore() {
  useReferenceManagerStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockCaps.mockReset();
  mockDomains.mockReset();
  mockSets.mockReset();
  resetStore();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("reference-manager-store — initial state", () => {
  it("starts empty with nothing selected", () => {
    const s = useReferenceManagerStore.getState();
    expect(s.domains).toEqual([]);
    expect(s.domainsLoading).toBe(false);
    expect(s.domainCapabilities).toEqual({});
    expect(s.setsMap).toEqual({});
    expect(s.setsLoading).toEqual({});
    expect(s.selectedDomainId).toBeNull();
    expect(s.selectedSetId).toBeNull();
    expect(s.expandedDomainIds).toEqual([]);
    expect(s.searchFilter).toBe("");
    expect(s.error).toBeNull();
  });
});

describe("reference-manager-store — loadDomains", () => {
  it("merges backend domains with the five synthetic domains, sorted by name", async () => {
    const zeta = makeDomain({ id: 1, name: "Zèta domaine", code: "Z.DOMAIN" });
    const alpha = makeDomain({ id: 2, name: "Alpha domaine", code: "A.DOMAIN" });
    mockDomains.mockResolvedValueOnce([zeta, alpha]);
    mockCaps.mockResolvedValue(makeCaps());

    await useReferenceManagerStore.getState().loadDomains();

    const { domains } = useReferenceManagerStore.getState();
    expect(domains).toHaveLength(7);
    expect(domains.map((d) => d.id)).toEqual(
      expect.arrayContaining([1, 2, ...SYNTHETIC_DOMAIN_IDS]),
    );
    const names = domains.map((d) => d.name);
    expect(names).toEqual([...names].sort((a, b) => a.localeCompare(b)));
    expect(domains[0]).toEqual(alpha);
  });

  it("drops superseded WORK.PRIORITY domains (case/whitespace-insensitive)", async () => {
    const keep = makeDomain({ id: 1 });
    const superseded = makeDomain({ id: 2, code: " work.priority ", name: "Priorités legacy" });
    mockDomains.mockResolvedValueOnce([keep, superseded]);
    mockCaps.mockResolvedValue(makeCaps());

    await useReferenceManagerStore.getState().loadDomains();

    const ids = useReferenceManagerStore.getState().domains.map((d) => d.id);
    expect(ids).toContain(1);
    expect(ids).not.toContain(2);
    expect(mockCaps).not.toHaveBeenCalledWith(2, null);
  });

  it("does not duplicate a synthetic domain the backend already returns", async () => {
    const backendWoTypes = makeDomain({
      id: WORK_ORDER_TYPES_DOMAIN_ID,
      code: "WO.WORK_ORDER_TYPES",
      name: "Types OT (backend)",
    });
    mockDomains.mockResolvedValueOnce([backendWoTypes]);
    mockCaps.mockResolvedValue(makeCaps());

    await useReferenceManagerStore.getState().loadDomains();

    const { domains } = useReferenceManagerStore.getState();
    const woTypes = domains.filter((d) => d.id === WORK_ORDER_TYPES_DOMAIN_ID);
    expect(woTypes).toEqual([backendWoTypes]);
    expect(domains).toHaveLength(5);
  });

  it("loads domain-level capabilities only for backend domains, skipping failures", async () => {
    const a = makeDomain({ id: 1 });
    const b = makeDomain({ id: 2, code: "B" });
    mockDomains.mockResolvedValueOnce([a, b]);
    const capsA = makeCaps({ category: "operational_dictionary" });
    mockCaps.mockImplementation((domainId) =>
      domainId === 1 ? Promise.resolve(capsA) : Promise.reject(new Error("no caps")),
    );

    await useReferenceManagerStore.getState().loadDomains();

    expect(mockCaps).toHaveBeenCalledTimes(2);
    expect(mockCaps).toHaveBeenCalledWith(1, null);
    expect(mockCaps).toHaveBeenCalledWith(2, null);
    const s = useReferenceManagerStore.getState();
    expect(s.domainCapabilities).toEqual({ 1: capsA });
    // a per-domain capability failure is not surfaced as a store error
    expect(s.error).toBeNull();
  });

  it("seeds setsMap with synthetic sets while preserving already-loaded sets", async () => {
    const existing = [makeSet({ id: 55, domain_id: 9 })];
    useReferenceManagerStore.setState({ setsMap: { 9: existing } });
    mockDomains.mockResolvedValueOnce([]);

    await useReferenceManagerStore.getState().loadDomains();

    const { setsMap } = useReferenceManagerStore.getState();
    expect(setsMap[9]).toEqual(existing);
    for (const [domainId, setId] of SYNTHETIC_PAIRS) {
      expect(setsMap[domainId]).toHaveLength(1);
      expect(setsMap[domainId]?.[0]?.id).toBe(setId);
      expect(setsMap[domainId]?.[0]?.status).toBe("published");
    }
  });

  it("sets domainsLoading while in flight and clears a previous error", async () => {
    const d = deferred<ReferenceDomain[]>();
    mockDomains.mockReturnValueOnce(d.promise);
    useReferenceManagerStore.setState({ error: "old" });

    const pending = useReferenceManagerStore.getState().loadDomains();
    expect(useReferenceManagerStore.getState().domainsLoading).toBe(true);
    expect(useReferenceManagerStore.getState().error).toBeNull();

    d.resolve([]);
    await pending;
    expect(useReferenceManagerStore.getState().domainsLoading).toBe(false);
  });

  it("sets error, leaves domains untouched and swallows the failure", async () => {
    const previous = [makeDomain()];
    useReferenceManagerStore.setState({ domains: previous });
    mockDomains.mockRejectedValueOnce({ code: "DB", message: "domains failed" });

    await expect(useReferenceManagerStore.getState().loadDomains()).resolves.toBeUndefined();

    const s = useReferenceManagerStore.getState();
    expect(s.error).toBe("domains failed");
    expect(s.domains).toEqual(previous);
    expect(s.domainsLoading).toBe(false);
    expect(mockCaps).not.toHaveBeenCalled();
  });
});

describe("reference-manager-store — loadSetsForDomain", () => {
  it.each(SYNTHETIC_PAIRS)(
    "synthetic domain %i gets its single published set without calling the backend",
    async (domainId, setId) => {
      await useReferenceManagerStore.getState().loadSetsForDomain(domainId);

      expect(mockSets).not.toHaveBeenCalled();
      const sets = useReferenceManagerStore.getState().setsMap[domainId];
      expect(sets?.map((s) => s.id)).toEqual([setId]);
      expect(useReferenceManagerStore.getState().setsLoading[domainId]).toBeUndefined();
    },
  );

  it("loads sets from the backend and stores them by domain", async () => {
    const sets = [makeSet({ id: 100 }), makeSet({ id: 101, version_no: 2, status: "draft" })];
    mockSets.mockResolvedValueOnce(sets);

    await useReferenceManagerStore.getState().loadSetsForDomain(1);

    expect(mockSets).toHaveBeenCalledWith(1);
    expect(useReferenceManagerStore.getState().setsMap[1]).toEqual(sets);
    expect(useReferenceManagerStore.getState().setsLoading[1]).toBe(false);
  });

  it("tracks setsLoading per domain and ignores duplicate calls while loading", async () => {
    const d = deferred<ReferenceSet[]>();
    mockSets.mockReturnValueOnce(d.promise);

    const first = useReferenceManagerStore.getState().loadSetsForDomain(1);
    expect(useReferenceManagerStore.getState().setsLoading).toEqual({ 1: true });

    await useReferenceManagerStore.getState().loadSetsForDomain(1);
    expect(mockSets).toHaveBeenCalledTimes(1);

    d.resolve([makeSet()]);
    await first;
    expect(useReferenceManagerStore.getState().setsLoading).toEqual({ 1: false });
  });

  it("sets error and clears the loading flag on failure (swallowed)", async () => {
    mockSets.mockRejectedValueOnce(new Error("sets failed"));

    await expect(useReferenceManagerStore.getState().loadSetsForDomain(1)).resolves.toBeUndefined();

    const s = useReferenceManagerStore.getState();
    expect(s.error).toBe("sets failed");
    expect(s.setsLoading[1]).toBe(false);
    expect(s.setsMap[1]).toBeUndefined();
  });

  it("auto-selects the latest published set for a selected operational dictionary", async () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "operational_dictionary" })],
      selectedDomainId: 1,
      selectedSetId: null,
    });
    mockSets.mockResolvedValueOnce([
      makeSet({ id: 100, version_no: 1, status: "published" }),
      makeSet({ id: 102, version_no: 3, status: "draft" }),
      makeSet({ id: 101, version_no: 2, status: "published" }),
    ]);

    await useReferenceManagerStore.getState().loadSetsForDomain(1);

    const s = useReferenceManagerStore.getState();
    expect(s.selectedSetId).toBe(101);
    expect(s.selectedDomainId).toBe(1);
    expect(s.expandedDomainIds).toContain(1);
  });

  it("does not auto-select when the dictionary has no published set", async () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "operational_dictionary" })],
      selectedDomainId: 1,
    });
    mockSets.mockResolvedValueOnce([makeSet({ status: "draft" })]);

    await useReferenceManagerStore.getState().loadSetsForDomain(1);

    expect(useReferenceManagerStore.getState().selectedSetId).toBeNull();
  });

  it("does not auto-select when a set is already selected", async () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "operational_dictionary" })],
      selectedDomainId: 1,
      selectedSetId: 999,
    });
    mockSets.mockResolvedValueOnce([makeSet({ id: 100 })]);

    await useReferenceManagerStore.getState().loadSetsForDomain(1);

    expect(useReferenceManagerStore.getState().selectedSetId).toBe(999);
  });

  it("does not auto-select when another domain is selected", async () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "operational_dictionary" })],
      selectedDomainId: 2,
    });
    mockSets.mockResolvedValueOnce([makeSet({ id: 100 })]);

    await useReferenceManagerStore.getState().loadSetsForDomain(1);

    expect(useReferenceManagerStore.getState().selectedDomainId).toBe(2);
    expect(useReferenceManagerStore.getState().selectedSetId).toBeNull();
  });

  it("does not auto-select for non-operational (controlled/system) catalogs", async () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "controlled_catalog" })],
      selectedDomainId: 1,
    });
    mockSets.mockResolvedValueOnce([makeSet({ id: 100 })]);

    await useReferenceManagerStore.getState().loadSetsForDomain(1);

    expect(useReferenceManagerStore.getState().selectedSetId).toBeNull();
  });
});

describe("reference-manager-store — selectDomain", () => {
  it("selects and expands the domain, clears set selection and eagerly loads sets", async () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1 })],
      selectedSetId: 50,
    });
    mockSets.mockResolvedValueOnce([makeSet()]);

    useReferenceManagerStore.getState().selectDomain(1);

    const s = useReferenceManagerStore.getState();
    expect(s.selectedDomainId).toBe(1);
    expect(s.selectedSetId).toBeNull();
    expect(s.expandedDomainIds).toEqual([1]);
    expect(mockSets).toHaveBeenCalledWith(1);
    await vi.waitFor(() => {
      expect(useReferenceManagerStore.getState().setsMap[1]).toHaveLength(1);
    });
  });

  it("does not duplicate an already-expanded domain", () => {
    useReferenceManagerStore.setState({
      expandedDomainIds: [1, 3],
      setsMap: { 1: [makeSet()] },
    });

    useReferenceManagerStore.getState().selectDomain(1);

    expect(useReferenceManagerStore.getState().expandedDomainIds).toEqual([1, 3]);
  });

  it("does not reload sets that are already cached", () => {
    useReferenceManagerStore.setState({ setsMap: { 1: [makeSet()] } });

    useReferenceManagerStore.getState().selectDomain(1);

    expect(mockSets).not.toHaveBeenCalled();
  });

  it("auto-selects the working published set of an operational dictionary from cache", () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "operational_dictionary" })],
      setsMap: {
        1: [makeSet({ id: 100, version_no: 1 }), makeSet({ id: 101, version_no: 2 })],
      },
    });

    useReferenceManagerStore.getState().selectDomain(1);

    expect(useReferenceManagerStore.getState().selectedSetId).toBe(101);
  });

  it("auto-selects the synthetic set of a synthetic dictionary after loadDomains", async () => {
    mockDomains.mockResolvedValueOnce([]);
    await useReferenceManagerStore.getState().loadDomains();

    useReferenceManagerStore.getState().selectDomain(INVENTORY_ARTICLE_FAMILY_DOMAIN_ID);

    expect(useReferenceManagerStore.getState().selectedSetId).toBe(INVENTORY_ARTICLE_FAMILY_SET_ID);
    expect(mockSets).not.toHaveBeenCalled();
  });

  it("leaves set selection empty for system catalogs even when sets are cached", () => {
    useReferenceManagerStore.setState({
      domains: [makeDomain({ id: 1, governance_category: "system_catalog" })],
      setsMap: { 1: [makeSet({ id: 100 })] },
      selectedSetId: 100,
    });

    useReferenceManagerStore.getState().selectDomain(1);

    expect(useReferenceManagerStore.getState().selectedSetId).toBeNull();
  });
});

describe("reference-manager-store — selectSet", () => {
  it("selects the set and its parent domain and expands the domain", () => {
    useReferenceManagerStore.getState().selectSet(100, 1);

    const s = useReferenceManagerStore.getState();
    expect(s.selectedSetId).toBe(100);
    expect(s.selectedDomainId).toBe(1);
    expect(s.expandedDomainIds).toEqual([1]);
    expect(mockSets).not.toHaveBeenCalled();
  });

  it("does not duplicate the domain in expandedDomainIds", () => {
    useReferenceManagerStore.setState({ expandedDomainIds: [1] });

    useReferenceManagerStore.getState().selectSet(100, 1);

    expect(useReferenceManagerStore.getState().expandedDomainIds).toEqual([1]);
  });
});

describe("reference-manager-store — toggleDomainExpanded", () => {
  it("expands a collapsed domain and loads its sets on first expand", async () => {
    mockSets.mockResolvedValueOnce([makeSet()]);

    useReferenceManagerStore.getState().toggleDomainExpanded(1);

    expect(useReferenceManagerStore.getState().expandedDomainIds).toEqual([1]);
    expect(mockSets).toHaveBeenCalledWith(1);
    await vi.waitFor(() => {
      expect(useReferenceManagerStore.getState().setsLoading[1]).toBe(false);
    });
  });

  it("expands without loading when sets are cached", () => {
    useReferenceManagerStore.setState({ setsMap: { 1: [] } });

    useReferenceManagerStore.getState().toggleDomainExpanded(1);

    expect(useReferenceManagerStore.getState().expandedDomainIds).toEqual([1]);
    expect(mockSets).not.toHaveBeenCalled();
  });

  it("collapses an expanded domain without loading and without changing selection", () => {
    useReferenceManagerStore.setState({
      expandedDomainIds: [1, 2],
      selectedDomainId: 1,
      selectedSetId: 100,
    });

    useReferenceManagerStore.getState().toggleDomainExpanded(1);

    const s = useReferenceManagerStore.getState();
    expect(s.expandedDomainIds).toEqual([2]);
    expect(s.selectedDomainId).toBe(1);
    expect(s.selectedSetId).toBe(100);
    expect(mockSets).not.toHaveBeenCalled();
  });
});

describe("reference-manager-store — simple setters", () => {
  it("setSearchFilter stores the filter text verbatim", () => {
    useReferenceManagerStore.getState().setSearchFilter("  pan ");

    expect(useReferenceManagerStore.getState().searchFilter).toBe("  pan ");
  });

  it("clearSelection clears domain and set selection but keeps expansion", () => {
    useReferenceManagerStore.setState({
      selectedDomainId: 1,
      selectedSetId: 100,
      expandedDomainIds: [1],
    });

    useReferenceManagerStore.getState().clearSelection();

    const s = useReferenceManagerStore.getState();
    expect(s.selectedDomainId).toBeNull();
    expect(s.selectedSetId).toBeNull();
    expect(s.expandedDomainIds).toEqual([1]);
  });

  it("clearError clears the error", () => {
    useReferenceManagerStore.setState({ error: "x" });

    useReferenceManagerStore.getState().clearError();

    expect(useReferenceManagerStore.getState().error).toBeNull();
  });
});
