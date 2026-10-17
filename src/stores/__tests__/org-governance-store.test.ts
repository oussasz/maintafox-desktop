/**
 * org-governance-store — behaviour tests.
 *
 * Publish-readiness validation, draft lineage reconcile, publish (including the
 * step-up passthrough and post-failure revalidation) and the audit timeline.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  listOrgChangeEvents,
  publishOrgModel,
  reconcileOrgDraftLineage,
  validateOrgModelForPublish,
} from "@/services/org-governance-service";
import type {
  OrgChangeEvent,
  OrgDraftLineageReconcileResult,
  OrgPublishValidationResult,
} from "@shared/ipc-types";

import { useOrgGovernanceStore } from "../org-governance-store";

vi.mock("@/services/org-governance-service", () => ({
  listOrgChangeEvents: vi.fn(),
  publishOrgModel: vi.fn(),
  reconcileOrgDraftLineage: vi.fn(),
  validateOrgModelForPublish: vi.fn(),
}));

const mockList = vi.mocked(listOrgChangeEvents);
const mockPublish = vi.mocked(publishOrgModel);
const mockReconcile = vi.mocked(reconcileOrgDraftLineage);
const mockValidate = vi.mocked(validateOrgModelForPublish);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeValidation(
  overrides: Partial<OrgPublishValidationResult> = {},
): OrgPublishValidationResult {
  return {
    model_id: 7,
    can_publish: true,
    issue_count: 0,
    blocking_count: 0,
    issues: [],
    remap_count: 0,
    ...overrides,
  };
}

function makeReconcile(
  overrides: Partial<OrgDraftLineageReconcileResult> = {},
): OrgDraftLineageReconcileResult {
  return { draft_model_id: 7, cloned_count: 2, ...overrides };
}

function makeEvent(overrides: Partial<OrgChangeEvent> = {}): OrgChangeEvent {
  return {
    id: 1,
    entity_kind: "org_node",
    entity_id: 10,
    change_type: "rename",
    before_json: '{"name":"A"}',
    after_json: '{"name":"B"}',
    preview_summary_json: null,
    changed_by_id: 3,
    changed_at: "2026-10-01T10:00:00Z",
    requires_step_up: false,
    apply_result: "applied",
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const initial = useOrgGovernanceStore.getState();

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockReset();
  mockPublish.mockReset();
  mockReconcile.mockReset();
  mockValidate.mockReset();
  useOrgGovernanceStore.setState(initial, true);
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("org-governance-store", () => {
  it("starts with empty state", () => {
    const s = useOrgGovernanceStore.getState();
    expect(s.publishValidation).toBeNull();
    expect(s.validationLoading).toBe(false);
    expect(s.reconcileLoading).toBe(false);
    expect(s.auditEvents).toEqual([]);
    expect(s.auditLoading).toBe(false);
    expect(s.error).toBeNull();
  });

  describe("loadPublishValidation", () => {
    it("stores the validation result for the given model", async () => {
      const v = makeValidation({ model_id: 4 });
      mockValidate.mockResolvedValueOnce(v);

      await useOrgGovernanceStore.getState().loadPublishValidation(4);

      expect(mockValidate).toHaveBeenCalledWith(4);
      expect(useOrgGovernanceStore.getState().publishValidation).toEqual(v);
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(false);
    });

    it("toggles validationLoading during the call and clears stale error", async () => {
      const d = deferred<OrgPublishValidationResult>();
      mockValidate.mockReturnValueOnce(d.promise);
      useOrgGovernanceStore.setState({ error: "stale" });

      const p = useOrgGovernanceStore.getState().loadPublishValidation(7);
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(true);
      expect(useOrgGovernanceStore.getState().error).toBeNull();

      d.resolve(makeValidation());
      await p;
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(false);
    });

    it("sets a formatted error (Validation failed: prefix stripped) and does not rethrow", async () => {
      mockValidate.mockRejectedValueOnce({
        code: "VALIDATION",
        message: "Validation failed: bad tree",
      });

      await expect(
        useOrgGovernanceStore.getState().loadPublishValidation(7),
      ).resolves.toBeUndefined();

      expect(useOrgGovernanceStore.getState().error).toBe("bad tree");
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(false);
    });

    it("joins uncoded IPC validation details into the error", async () => {
      mockValidate.mockRejectedValueOnce({
        code: "VALIDATION",
        message: "Validation failed",
        details: [{ message: "first" }, { message: "second" }],
      });

      await useOrgGovernanceStore.getState().loadPublishValidation(7);

      expect(useOrgGovernanceStore.getState().error).toBe("first; second");
    });
  });

  describe("reconcileDraftLineage", () => {
    it("reconciles then revalidates the same draft model", async () => {
      mockReconcile.mockResolvedValueOnce(makeReconcile());
      const v = makeValidation({ remap_count: 2 });
      mockValidate.mockResolvedValueOnce(v);

      await useOrgGovernanceStore.getState().reconcileDraftLineage(7);

      expect(mockReconcile).toHaveBeenCalledWith(7);
      expect(mockValidate).toHaveBeenCalledWith(7);
      expect(mockReconcile.mock.invocationCallOrder[0]).toBeLessThan(
        mockValidate.mock.invocationCallOrder[0] ?? 0,
      );
      expect(useOrgGovernanceStore.getState().publishValidation).toEqual(v);
      expect(useOrgGovernanceStore.getState().reconcileLoading).toBe(false);
    });

    it("sets reconcileLoading while in flight (not validationLoading)", async () => {
      const d = deferred<OrgDraftLineageReconcileResult>();
      mockReconcile.mockReturnValueOnce(d.promise);
      mockValidate.mockResolvedValueOnce(makeValidation());

      const p = useOrgGovernanceStore.getState().reconcileDraftLineage(7);
      expect(useOrgGovernanceStore.getState().reconcileLoading).toBe(true);
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(false);

      d.resolve(makeReconcile());
      await p;
      expect(useOrgGovernanceStore.getState().reconcileLoading).toBe(false);
    });

    it("on reconcile failure sets error, disables publish and skips revalidation", async () => {
      useOrgGovernanceStore.setState({ publishValidation: makeValidation({ can_publish: true }) });
      mockReconcile.mockRejectedValueOnce({ code: "CONFLICT", message: "lineage broken" });

      await expect(
        useOrgGovernanceStore.getState().reconcileDraftLineage(7),
      ).resolves.toBeUndefined();

      const s = useOrgGovernanceStore.getState();
      expect(s.error).toBe("lineage broken");
      expect(s.publishValidation?.can_publish).toBe(false);
      expect(s.publishValidation?.model_id).toBe(7);
      expect(s.reconcileLoading).toBe(false);
      expect(mockValidate).not.toHaveBeenCalled();
    });

    it("on failure with no prior validation leaves publishValidation null", async () => {
      mockReconcile.mockResolvedValueOnce(makeReconcile());
      mockValidate.mockRejectedValueOnce(new Error("validate down"));

      await useOrgGovernanceStore.getState().reconcileDraftLineage(7);

      expect(useOrgGovernanceStore.getState().error).toBe("validate down");
      expect(useOrgGovernanceStore.getState().publishValidation).toBeNull();
    });
  });

  describe("publishModel", () => {
    it("stores the publish result and clears loading", async () => {
      const v = makeValidation({ can_publish: false, issue_count: 0 });
      mockPublish.mockResolvedValueOnce(v);

      await useOrgGovernanceStore.getState().publishModel(7);

      expect(mockPublish).toHaveBeenCalledWith(7);
      expect(useOrgGovernanceStore.getState().publishValidation).toEqual(v);
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(false);
      expect(mockValidate).not.toHaveBeenCalled();
    });

    it("uses validationLoading while publishing", async () => {
      const d = deferred<OrgPublishValidationResult>();
      mockPublish.mockReturnValueOnce(d.promise);

      const p = useOrgGovernanceStore.getState().publishModel(7);
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(true);

      d.resolve(makeValidation());
      await p;
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(false);
    });

    it("rethrows STEP_UP_REQUIRED IPC errors without setting error or touching validation", async () => {
      const current = makeValidation({ can_publish: true });
      useOrgGovernanceStore.setState({ publishValidation: current });
      const stepUp = { code: "STEP_UP_REQUIRED", message: "Re-authenticate" };
      mockPublish.mockRejectedValueOnce(stepUp);

      await expect(useOrgGovernanceStore.getState().publishModel(7)).rejects.toBe(stepUp);

      const s = useOrgGovernanceStore.getState();
      expect(s.error).toBeNull();
      expect(s.publishValidation).toBe(current);
      expect(s.validationLoading).toBe(false);
      expect(mockValidate).not.toHaveBeenCalled();
    });

    it("also rethrows errors mentioning StepUpRequired", async () => {
      const stepUp = { kind: "StepUpRequired" };
      mockPublish.mockRejectedValueOnce(stepUp);

      await expect(useOrgGovernanceStore.getState().publishModel(7)).rejects.toBe(stepUp);
      expect(useOrgGovernanceStore.getState().error).toBeNull();
    });

    it("rethrows a plain string step-up rejection", async () => {
      mockPublish.mockRejectedValueOnce("STEP_UP_REQUIRED");
      await expect(useOrgGovernanceStore.getState().publishModel(7)).rejects.toBe(
        "STEP_UP_REQUIRED",
      );
    });

    it("rethrows an Error instance whose message is STEP_UP_REQUIRED", async () => {
      const stepUp = new Error("STEP_UP_REQUIRED");
      mockPublish.mockRejectedValueOnce(stepUp);
      mockValidate.mockResolvedValueOnce(makeValidation());

      await expect(useOrgGovernanceStore.getState().publishModel(7)).rejects.toBe(stepUp);
    });

    it("on ordinary failure sets error, forces can_publish=false, then revalidates", async () => {
      useOrgGovernanceStore.setState({ publishValidation: makeValidation({ can_publish: true }) });
      mockPublish.mockRejectedValueOnce({
        code: "VALIDATION",
        message: "Validation failed: cycle",
      });
      const fresh = makeValidation({ can_publish: false, blocking_count: 1, issue_count: 1 });
      const d = deferred<OrgPublishValidationResult>();
      mockValidate.mockReturnValueOnce(d.promise);

      const p = useOrgGovernanceStore.getState().publishModel(7);
      // Wait until the store reaches the revalidation step.
      await vi.waitFor(() => {
        expect(mockValidate).toHaveBeenCalledWith(7);
      });
      // Publish is disabled while the refresh is in flight.
      expect(useOrgGovernanceStore.getState().publishValidation?.can_publish).toBe(false);
      expect(useOrgGovernanceStore.getState().error).toBe("cycle");
      expect(useOrgGovernanceStore.getState().validationLoading).toBe(true);

      d.resolve(fresh);
      await expect(p).resolves.toBeUndefined();

      const s = useOrgGovernanceStore.getState();
      expect(s.publishValidation).toEqual(fresh);
      expect(s.error).toBe("cycle");
      expect(s.validationLoading).toBe(false);
    });

    it("keeps the original error and disabled publish when the revalidation also fails", async () => {
      useOrgGovernanceStore.setState({ publishValidation: makeValidation({ can_publish: true }) });
      mockPublish.mockRejectedValueOnce({ code: "DB", message: "publish failed" });
      mockValidate.mockRejectedValueOnce({ code: "DB", message: "validate failed" });

      await expect(useOrgGovernanceStore.getState().publishModel(7)).resolves.toBeUndefined();

      const s = useOrgGovernanceStore.getState();
      expect(s.error).toBe("publish failed");
      expect(s.publishValidation?.can_publish).toBe(false);
      expect(s.validationLoading).toBe(false);
    });

    it("with no prior validation, a failed publish stores the revalidation result", async () => {
      mockPublish.mockRejectedValueOnce(new Error("nope"));
      const fresh = makeValidation({ can_publish: false });
      mockValidate.mockResolvedValueOnce(fresh);

      await useOrgGovernanceStore.getState().publishModel(7);

      expect(useOrgGovernanceStore.getState().error).toBe("nope");
      expect(useOrgGovernanceStore.getState().publishValidation).toEqual(fresh);
    });
  });

  describe("loadAuditEvents", () => {
    it("forwards limit / entity filters and stores events", async () => {
      const events = [makeEvent(), makeEvent({ id: 2, change_type: "move" })];
      mockList.mockResolvedValueOnce(events);

      await useOrgGovernanceStore.getState().loadAuditEvents(50, "org_node", 10);

      expect(mockList).toHaveBeenCalledWith(50, "org_node", 10);
      expect(useOrgGovernanceStore.getState().auditEvents).toEqual(events);
      expect(useOrgGovernanceStore.getState().auditLoading).toBe(false);
    });

    it("passes undefined filters through when omitted", async () => {
      mockList.mockResolvedValueOnce([]);
      await useOrgGovernanceStore.getState().loadAuditEvents();
      expect(mockList).toHaveBeenCalledWith(undefined, undefined, undefined);
    });

    it("sets auditLoading while in flight", async () => {
      const d = deferred<OrgChangeEvent[]>();
      mockList.mockReturnValueOnce(d.promise);

      const p = useOrgGovernanceStore.getState().loadAuditEvents(10);
      expect(useOrgGovernanceStore.getState().auditLoading).toBe(true);
      d.resolve([]);
      await p;
      expect(useOrgGovernanceStore.getState().auditLoading).toBe(false);
    });

    it("keeps an audit failure apart from the publish error, keeps previous events", async () => {
      const previous = [makeEvent()];
      const failure = { code: "DB", message: "audit unavailable" };
      useOrgGovernanceStore.setState({ auditEvents: previous });
      mockList.mockRejectedValueOnce(failure);

      await expect(useOrgGovernanceStore.getState().loadAuditEvents()).resolves.toBeUndefined();

      const s = useOrgGovernanceStore.getState();
      // A history outage must not block publishing (publish reads `error`).
      expect(s.error).toBeNull();
      expect(s.auditError).toBe(failure);
      expect(s.auditEvents).toBe(previous);
      expect(s.auditLoading).toBe(false);
    });
  });

  it("a successful audit reload clears the previous audit failure", async () => {
    useOrgGovernanceStore.setState({ auditError: { code: "DB" } });
    mockList.mockResolvedValueOnce([]);
    await useOrgGovernanceStore.getState().loadAuditEvents();
    expect(useOrgGovernanceStore.getState().auditError).toBeNull();
  });

  it("clearError resets error to null", () => {
    useOrgGovernanceStore.setState({ error: "x" });
    useOrgGovernanceStore.getState().clearError();
    expect(useOrgGovernanceStore.getState().error).toBeNull();
  });
});
