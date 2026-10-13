/**
 * reference-governance-store — behaviour tests.
 *
 * Mocks only the `@/services/reference-service` boundary.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  computeRefPublishReadiness,
  createReferenceValue,
  deactivateReferenceValue,
  governedPublishReferenceSet,
  listReferenceValues,
  previewRefPublishImpact,
  reactivateReferenceValue,
  updateReferenceValue,
} from "@/services/reference-service";
import type {
  CreateReferenceValuePayload,
  ReferenceImpactSummary,
  ReferencePublishReadiness,
  ReferencePublishResult,
  ReferenceSet,
  ReferenceValue,
} from "@shared/ipc-types";

import { useReferenceGovernanceStore } from "../reference-governance-store";

vi.mock("@/services/reference-service", () => ({
  computeRefPublishReadiness: vi.fn(),
  createReferenceValue: vi.fn(),
  deactivateReferenceValue: vi.fn(),
  governedPublishReferenceSet: vi.fn(),
  listReferenceValues: vi.fn(),
  previewRefPublishImpact: vi.fn(),
  reactivateReferenceValue: vi.fn(),
  updateReferenceValue: vi.fn(),
}));

const mockReadiness = vi.mocked(computeRefPublishReadiness);
const mockCreate = vi.mocked(createReferenceValue);
const mockDeactivate = vi.mocked(deactivateReferenceValue);
const mockPublish = vi.mocked(governedPublishReferenceSet);
const mockList = vi.mocked(listReferenceValues);
const mockImpact = vi.mocked(previewRefPublishImpact);
const mockReactivate = vi.mocked(reactivateReferenceValue);
const mockUpdate = vi.mocked(updateReferenceValue);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeValue(overrides: Partial<ReferenceValue> = {}): ReferenceValue {
  return {
    id: 1,
    set_id: 10,
    parent_id: null,
    code: "MECH",
    label: "Mécanique",
    description: null,
    sort_order: 1,
    color_hex: null,
    icon_name: null,
    semantic_tag: null,
    external_code: null,
    is_active: true,
    metadata_json: null,
    ...overrides,
  };
}

function makeReadiness(
  overrides: Partial<ReferencePublishReadiness> = {},
): ReferencePublishReadiness {
  return {
    set_id: 10,
    domain_id: 2,
    is_ready: true,
    is_protected: false,
    issues: [],
    impact_preview_required: false,
    impact_preview_available: true,
    ...overrides,
  };
}

function makeImpact(overrides: Partial<ReferenceImpactSummary> = {}): ReferenceImpactSummary {
  return {
    set_id: 10,
    domain_id: 2,
    domain_code: "FAILURE.CAUSE",
    removed_codes: ["OLD"],
    deactivated_codes: [],
    total_affected: 3,
    dimensions: [],
    computed_at: "2026-10-12T10:00:00Z",
    ...overrides,
  };
}

function makeSet(overrides: Partial<ReferenceSet> = {}): ReferenceSet {
  return {
    id: 10,
    domain_id: 2,
    version_no: 2,
    status: "published",
    effective_from: null,
    created_by_id: 1,
    created_at: "2026-10-01T10:00:00Z",
    published_at: "2026-10-12T10:00:00Z",
    ...overrides,
  };
}

function makePublishResult(
  overrides: Partial<ReferencePublishResult> = {},
): ReferencePublishResult {
  return {
    set: makeSet(),
    superseded_set_id: 9,
    readiness: makeReadiness(),
    ...overrides,
  };
}

function makeCreatePayload(
  overrides: Partial<CreateReferenceValuePayload> = {},
): CreateReferenceValuePayload {
  return { set_id: 10, code: "ELEC", label: "Électrique", ...overrides };
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

const initialState = useReferenceGovernanceStore.getState();

function resetStore() {
  useReferenceGovernanceStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const m of [
    mockReadiness,
    mockCreate,
    mockDeactivate,
    mockPublish,
    mockList,
    mockImpact,
    mockReactivate,
    mockUpdate,
  ]) {
    m.mockReset();
  }
  resetStore();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("reference-governance-store — initial state", () => {
  it("starts with no values, readiness or impact and all flags off", () => {
    const s = useReferenceGovernanceStore.getState();
    expect(s.values).toEqual([]);
    expect(s.valuesLoading).toBe(false);
    expect(s.editingValueId).toBeNull();
    expect(s.newValueDraft).toBeNull();
    expect(s.savingValue).toBe(false);
    expect(s.readiness).toBeNull();
    expect(s.readinessLoading).toBe(false);
    expect(s.impactSummary).toBeNull();
    expect(s.impactLoading).toBe(false);
    expect(s.error).toBeNull();
  });
});

describe("reference-governance-store — loadValues", () => {
  it("loads values for the set and resets edit/draft state", async () => {
    const values = [makeValue(), makeValue({ id: 2, code: "ELEC" })];
    mockList.mockResolvedValueOnce(values);
    useReferenceGovernanceStore.setState({
      editingValueId: 1,
      newValueDraft: { code: "X" },
      error: "old",
    });

    await useReferenceGovernanceStore.getState().loadValues(10);

    expect(mockList).toHaveBeenCalledWith(10);
    const s = useReferenceGovernanceStore.getState();
    expect(s.values).toEqual(values);
    expect(s.editingValueId).toBeNull();
    expect(s.newValueDraft).toBeNull();
    expect(s.error).toBeNull();
    expect(s.valuesLoading).toBe(false);
  });

  it("sets valuesLoading while in flight", async () => {
    const d = deferred<ReferenceValue[]>();
    mockList.mockReturnValueOnce(d.promise);

    const pending = useReferenceGovernanceStore.getState().loadValues(10);
    expect(useReferenceGovernanceStore.getState().valuesLoading).toBe(true);

    d.resolve([]);
    await pending;
    expect(useReferenceGovernanceStore.getState().valuesLoading).toBe(false);
  });

  it("sets error, keeps previous values and does not rethrow on failure", async () => {
    const previous = [makeValue()];
    useReferenceGovernanceStore.setState({ values: previous });
    mockList.mockRejectedValueOnce({ code: "DB", message: "cannot list" });

    await expect(useReferenceGovernanceStore.getState().loadValues(10)).resolves.toBeUndefined();

    const s = useReferenceGovernanceStore.getState();
    expect(s.error).toBe("cannot list");
    expect(s.values).toEqual(previous);
    expect(s.valuesLoading).toBe(false);
  });
});

describe("reference-governance-store — saveValue", () => {
  it("replaces the updated value in place and exits edit mode", async () => {
    const a = makeValue({ id: 1 });
    const b = makeValue({ id: 2, code: "ELEC" });
    useReferenceGovernanceStore.setState({ values: [a, b], editingValueId: 2 });
    const updated = makeValue({ id: 2, code: "ELEC", label: "Electrical" });
    mockUpdate.mockResolvedValueOnce(updated);

    await useReferenceGovernanceStore.getState().saveValue(2, { label: "Electrical" });

    expect(mockUpdate).toHaveBeenCalledWith(2, { label: "Electrical" });
    const s = useReferenceGovernanceStore.getState();
    expect(s.values).toEqual([a, updated]);
    expect(s.editingValueId).toBeNull();
    expect(s.savingValue).toBe(false);
  });

  it("sets savingValue while in flight", async () => {
    const d = deferred<ReferenceValue>();
    mockUpdate.mockReturnValueOnce(d.promise);
    useReferenceGovernanceStore.setState({ values: [makeValue()] });

    const pending = useReferenceGovernanceStore.getState().saveValue(1, { label: "x" });
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(true);

    d.resolve(makeValue({ label: "x" }));
    await pending;
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(false);
  });

  it("sets error, stays in edit mode and rethrows on failure", async () => {
    const original = [makeValue()];
    useReferenceGovernanceStore.setState({ values: original, editingValueId: 1 });
    const failure = { code: "GOVERNANCE", message: "read only" };
    mockUpdate.mockRejectedValueOnce(failure);

    await expect(
      useReferenceGovernanceStore.getState().saveValue(1, { label: "x" }),
    ).rejects.toEqual(failure);

    const s = useReferenceGovernanceStore.getState();
    expect(s.error).toBe("read only");
    expect(s.editingValueId).toBe(1);
    expect(s.values).toEqual(original);
    expect(s.savingValue).toBe(false);
  });
});

describe("reference-governance-store — addValue", () => {
  it("prepends the created value and clears the draft", async () => {
    const existing = makeValue({ id: 1 });
    useReferenceGovernanceStore.setState({ values: [existing], newValueDraft: { code: "ELEC" } });
    const created = makeValue({ id: 3, code: "ELEC" });
    mockCreate.mockResolvedValueOnce(created);
    const payload = makeCreatePayload();

    await useReferenceGovernanceStore.getState().addValue(payload);

    expect(mockCreate).toHaveBeenCalledWith(payload);
    const s = useReferenceGovernanceStore.getState();
    expect(s.values).toEqual([created, existing]);
    expect(s.newValueDraft).toBeNull();
    expect(s.savingValue).toBe(false);
  });

  it("sets savingValue while in flight", async () => {
    const d = deferred<ReferenceValue>();
    mockCreate.mockReturnValueOnce(d.promise);

    const pending = useReferenceGovernanceStore.getState().addValue(makeCreatePayload());
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(true);

    d.resolve(makeValue({ id: 4 }));
    await pending;
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(false);
  });

  it("sets error, keeps the draft and swallows the error (unlike saveValue)", async () => {
    const draft = { code: "DUP" };
    useReferenceGovernanceStore.setState({ newValueDraft: draft });
    mockCreate.mockRejectedValueOnce(new Error("duplicate code"));

    await expect(
      useReferenceGovernanceStore.getState().addValue(makeCreatePayload({ code: "DUP" })),
    ).resolves.toBeUndefined();

    const s = useReferenceGovernanceStore.getState();
    expect(s.error).toBe("duplicate code");
    expect(s.newValueDraft).toEqual(draft);
    expect(s.values).toEqual([]);
    expect(s.savingValue).toBe(false);
  });
});

describe("reference-governance-store — removeValue", () => {
  it("soft-deactivates and keeps the row in place as inactive", async () => {
    const a = makeValue({ id: 1 });
    const b = makeValue({ id: 2, code: "ELEC" });
    useReferenceGovernanceStore.setState({ values: [a, b] });
    const inactive = makeValue({ id: 1, is_active: false });
    mockDeactivate.mockResolvedValueOnce(inactive);

    await useReferenceGovernanceStore.getState().removeValue(1);

    expect(mockDeactivate).toHaveBeenCalledWith(1);
    expect(useReferenceGovernanceStore.getState().values).toEqual([inactive, b]);
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(false);
  });

  it("sets error and swallows failures", async () => {
    const values = [makeValue()];
    useReferenceGovernanceStore.setState({ values });
    mockDeactivate.mockRejectedValueOnce("in use");

    await expect(useReferenceGovernanceStore.getState().removeValue(1)).resolves.toBeUndefined();

    expect(useReferenceGovernanceStore.getState().error).toBe("in use");
    expect(useReferenceGovernanceStore.getState().values).toEqual(values);
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(false);
  });
});

describe("reference-governance-store — setValueActive", () => {
  it("active=true calls reactivate and replaces the row", async () => {
    useReferenceGovernanceStore.setState({ values: [makeValue({ is_active: false })] });
    const active = makeValue({ is_active: true });
    mockReactivate.mockResolvedValueOnce(active);

    await useReferenceGovernanceStore.getState().setValueActive(1, true);

    expect(mockReactivate).toHaveBeenCalledWith(1);
    expect(mockDeactivate).not.toHaveBeenCalled();
    expect(useReferenceGovernanceStore.getState().values).toEqual([active]);
  });

  it("active=false calls deactivate and replaces the row", async () => {
    useReferenceGovernanceStore.setState({ values: [makeValue()] });
    const inactive = makeValue({ is_active: false });
    mockDeactivate.mockResolvedValueOnce(inactive);

    await useReferenceGovernanceStore.getState().setValueActive(1, false);

    expect(mockDeactivate).toHaveBeenCalledWith(1);
    expect(mockReactivate).not.toHaveBeenCalled();
    expect(useReferenceGovernanceStore.getState().values).toEqual([inactive]);
  });

  it("sets savingValue while in flight", async () => {
    const d = deferred<ReferenceValue>();
    mockReactivate.mockReturnValueOnce(d.promise);

    const pending = useReferenceGovernanceStore.getState().setValueActive(1, true);
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(true);

    d.resolve(makeValue());
    await pending;
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(false);
  });

  it("sets error and swallows failures", async () => {
    mockReactivate.mockRejectedValueOnce({ message: "parent inactive" });

    await expect(
      useReferenceGovernanceStore.getState().setValueActive(1, true),
    ).resolves.toBeUndefined();

    expect(useReferenceGovernanceStore.getState().error).toBe("parent inactive");
    expect(useReferenceGovernanceStore.getState().savingValue).toBe(false);
  });
});

describe("reference-governance-store — edit/draft toggles", () => {
  it("setEditingValueId enters edit mode and discards any new-value draft", () => {
    useReferenceGovernanceStore.setState({ newValueDraft: { code: "X" } });

    useReferenceGovernanceStore.getState().setEditingValueId(5);

    expect(useReferenceGovernanceStore.getState().editingValueId).toBe(5);
    expect(useReferenceGovernanceStore.getState().newValueDraft).toBeNull();
  });

  it("setEditingValueId(null) leaves edit mode", () => {
    useReferenceGovernanceStore.setState({ editingValueId: 5 });

    useReferenceGovernanceStore.getState().setEditingValueId(null);

    expect(useReferenceGovernanceStore.getState().editingValueId).toBeNull();
  });

  it("setNewValueDraft stores the draft and cancels inline editing", () => {
    useReferenceGovernanceStore.setState({ editingValueId: 5 });

    useReferenceGovernanceStore.getState().setNewValueDraft({ set_id: 10, code: "N" });

    expect(useReferenceGovernanceStore.getState().newValueDraft).toEqual({ set_id: 10, code: "N" });
    expect(useReferenceGovernanceStore.getState().editingValueId).toBeNull();
  });

  it("setNewValueDraft(null) discards the draft", () => {
    useReferenceGovernanceStore.setState({ newValueDraft: { code: "N" } });

    useReferenceGovernanceStore.getState().setNewValueDraft(null);

    expect(useReferenceGovernanceStore.getState().newValueDraft).toBeNull();
  });
});

describe("reference-governance-store — loadReadiness", () => {
  it("stores the readiness result", async () => {
    const readiness = makeReadiness({
      is_ready: false,
      issues: [{ check: "duplicates", message: "dup code", severity: "error" }],
    });
    mockReadiness.mockResolvedValueOnce(readiness);

    await useReferenceGovernanceStore.getState().loadReadiness(10);

    expect(mockReadiness).toHaveBeenCalledWith(10);
    expect(useReferenceGovernanceStore.getState().readiness).toEqual(readiness);
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(false);
  });

  it("sets readinessLoading while in flight", async () => {
    const d = deferred<ReferencePublishReadiness>();
    mockReadiness.mockReturnValueOnce(d.promise);

    const pending = useReferenceGovernanceStore.getState().loadReadiness(10);
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(true);

    d.resolve(makeReadiness());
    await pending;
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(false);
  });

  it("sets error, keeps prior readiness and swallows failures", async () => {
    const prior = makeReadiness();
    useReferenceGovernanceStore.setState({ readiness: prior });
    mockReadiness.mockRejectedValueOnce(new Error("readiness failed"));

    await expect(useReferenceGovernanceStore.getState().loadReadiness(10)).resolves.toBeUndefined();

    expect(useReferenceGovernanceStore.getState().error).toBe("readiness failed");
    expect(useReferenceGovernanceStore.getState().readiness).toEqual(prior);
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(false);
  });
});

describe("reference-governance-store — loadImpact", () => {
  it("stores the impact summary", async () => {
    const impact = makeImpact();
    mockImpact.mockResolvedValueOnce(impact);

    await useReferenceGovernanceStore.getState().loadImpact(10);

    expect(mockImpact).toHaveBeenCalledWith(10);
    expect(useReferenceGovernanceStore.getState().impactSummary).toEqual(impact);
    expect(useReferenceGovernanceStore.getState().impactLoading).toBe(false);
  });

  it("sets impactLoading while in flight (independent of readinessLoading)", async () => {
    const d = deferred<ReferenceImpactSummary>();
    mockImpact.mockReturnValueOnce(d.promise);

    const pending = useReferenceGovernanceStore.getState().loadImpact(10);
    expect(useReferenceGovernanceStore.getState().impactLoading).toBe(true);
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(false);

    d.resolve(makeImpact());
    await pending;
    expect(useReferenceGovernanceStore.getState().impactLoading).toBe(false);
  });

  it("sets error and swallows failures", async () => {
    mockImpact.mockRejectedValueOnce({ code: "X", message: "impact failed" });

    await expect(useReferenceGovernanceStore.getState().loadImpact(10)).resolves.toBeUndefined();

    expect(useReferenceGovernanceStore.getState().error).toBe("impact failed");
    expect(useReferenceGovernanceStore.getState().impactSummary).toBeNull();
    expect(useReferenceGovernanceStore.getState().impactLoading).toBe(false);
  });
});

describe("reference-governance-store — publish", () => {
  it("publishes and stores the readiness returned by the publish result", async () => {
    const readiness = makeReadiness({ is_ready: true, set_id: 10 });
    mockPublish.mockResolvedValueOnce(makePublishResult({ readiness }));

    await useReferenceGovernanceStore.getState().publish(10);

    expect(mockPublish).toHaveBeenCalledWith(10);
    expect(mockReadiness).not.toHaveBeenCalled();
    expect(useReferenceGovernanceStore.getState().readiness).toEqual(readiness);
    expect(useReferenceGovernanceStore.getState().error).toBeNull();
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(false);
  });

  it("uses readinessLoading as the busy flag while publishing", async () => {
    const d = deferred<ReferencePublishResult>();
    mockPublish.mockReturnValueOnce(d.promise);
    useReferenceGovernanceStore.setState({ error: "old" });

    const pending = useReferenceGovernanceStore.getState().publish(10);
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(true);
    expect(useReferenceGovernanceStore.getState().error).toBeNull();

    d.resolve(makePublishResult());
    await pending;
    expect(useReferenceGovernanceStore.getState().readinessLoading).toBe(false);
  });

  it("on failure sets the publish error and refreshes readiness to show blockers (swallowed)", async () => {
    mockPublish.mockRejectedValueOnce({ code: "BLOCKED", message: "set not ready" });
    const blockers = makeReadiness({
      is_ready: false,
      issues: [{ check: "empty_set", message: "no values", severity: "error" }],
    });
    mockReadiness.mockResolvedValueOnce(blockers);

    await expect(useReferenceGovernanceStore.getState().publish(10)).resolves.toBeUndefined();

    expect(mockReadiness).toHaveBeenCalledWith(10);
    const s = useReferenceGovernanceStore.getState();
    expect(s.error).toBe("set not ready");
    expect(s.readiness).toEqual(blockers);
    expect(s.readinessLoading).toBe(false);
  });

  it("keeps the original publish error when the readiness refresh also fails", async () => {
    const prior = makeReadiness({ is_ready: true });
    useReferenceGovernanceStore.setState({ readiness: prior });
    mockPublish.mockRejectedValueOnce(new Error("publish failed"));
    mockReadiness.mockRejectedValueOnce(new Error("refresh failed"));

    await expect(useReferenceGovernanceStore.getState().publish(10)).resolves.toBeUndefined();

    const s = useReferenceGovernanceStore.getState();
    expect(s.error).toBe("publish failed");
    expect(s.readiness).toEqual(prior);
    expect(s.readinessLoading).toBe(false);
  });
});

describe("reference-governance-store — clearError", () => {
  it("clears the error", () => {
    useReferenceGovernanceStore.setState({ error: "x" });

    useReferenceGovernanceStore.getState().clearError();

    expect(useReferenceGovernanceStore.getState().error).toBeNull();
  });
});
