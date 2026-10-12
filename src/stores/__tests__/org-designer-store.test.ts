/**
 * org-designer-store — behaviour tests.
 *
 * Mocks only the org-designer-service boundary.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { getOrgDesignerSnapshot, previewOrgChange } from "@/services/org-designer-service";
import type {
  OrgDesignerNodeRow,
  OrgDesignerSnapshot,
  OrgImpactPreview,
  PreviewOrgChangePayload,
} from "@shared/ipc-types";

import { isOrgStructureDesignMode, useOrgDesignerStore } from "../org-designer-store";

vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: vi.fn(),
  previewOrgChange: vi.fn(),
}));

const mockGetSnapshot = vi.mocked(getOrgDesignerSnapshot);
const mockPreview = vi.mocked(previewOrgChange);

// ── Fixtures ─────────────────────────────────────────────────────────────────

function makeNode(overrides: Partial<OrgDesignerNodeRow> = {}): OrgDesignerNodeRow {
  return {
    node_id: 1,
    parent_id: null,
    ancestor_path: "/1/",
    depth: 0,
    code: "SITE-01",
    name: "Main site",
    status: "active",
    row_version: 1,
    node_type_id: 10,
    node_type_code: "SITE",
    node_type_label: "Site",
    can_host_assets: true,
    can_own_work: true,
    can_carry_cost_center: true,
    can_aggregate_kpis: true,
    can_receive_permits: false,
    child_count: 0,
    active_responsibility_count: 0,
    active_binding_count: 0,
    ...overrides,
  };
}

function makeSnapshot(overrides: Partial<OrgDesignerSnapshot> = {}): OrgDesignerSnapshot {
  return {
    active_model_id: 1,
    active_model_version: 3,
    draft_model_id: null,
    draft_model_version: null,
    display_model_id: 1,
    nodes: [makeNode()],
    ...overrides,
  };
}

/** Snapshot where both a published model and a draft model exist. */
function makeBothSnapshot(overrides: Partial<OrgDesignerSnapshot> = {}): OrgDesignerSnapshot {
  return makeSnapshot({ draft_model_id: 2, draft_model_version: 4, ...overrides });
}

function makePreview(overrides: Partial<OrgImpactPreview> = {}): OrgImpactPreview {
  return {
    action: "MoveNode",
    subject_node_id: 5,
    affected_node_count: 3,
    descendant_count: 2,
    active_responsibility_count: 1,
    active_binding_count: 0,
    blockers: [],
    warnings: [
      {
        code: "ORG_MOVE_RESP",
        severity: "warning",
        message: "Responsibilities will move",
        related_id: 5,
      },
    ],
    dependencies: [{ domain: "assets", status: "ok", count: 4, note: null }],
    ...overrides,
  };
}

function makePayload(overrides: Partial<PreviewOrgChangePayload> = {}): PreviewOrgChangePayload {
  return { action: "MoveNode", node_id: 5, new_parent_id: 1, ...overrides };
}

function deferred<T>() {
  let resolve: (v: T) => void = () => undefined;
  let reject: (e: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const initialState = useOrgDesignerStore.getState();

function resetStore() {
  useOrgDesignerStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockGetSnapshot.mockReset();
  mockPreview.mockReset();
  resetStore();
});

// ── Tests ────────────────────────────────────────────────────────────────────

describe("isOrgStructureDesignMode", () => {
  it("is true only when a draft exists and mode is draft", () => {
    expect(isOrgStructureDesignMode(makeBothSnapshot(), "draft")).toBe(true);
    expect(isOrgStructureDesignMode(makeBothSnapshot(), "published")).toBe(false);
    expect(isOrgStructureDesignMode(makeSnapshot(), "draft")).toBe(false);
    expect(isOrgStructureDesignMode(null, "draft")).toBe(false);
  });
});

describe("org-designer-store", () => {
  it("starts in published mode with empty state", () => {
    const s = useOrgDesignerStore.getState();
    expect(s.snapshot).toBeNull();
    expect(s.workspaceMode).toBe("published");
    expect(s.filterText).toBe("");
    expect(s.statusFilter).toBeNull();
    expect(s.typeFilter).toBeNull();
    expect(s.selectedNodeId).toBeNull();
    expect(s.preview).toBeNull();
    expect(s.previewPayload).toBeNull();
    expect(s.previewOpen).toBe(false);
    expect(s.loading).toBe(false);
    expect(s.previewLoading).toBe(false);
    expect(s.error).toBeNull();
  });

  describe("loadSnapshot", () => {
    it("sets loading while in flight and stores the snapshot", async () => {
      const d = deferred<OrgDesignerSnapshot>();
      mockGetSnapshot.mockReturnValueOnce(d.promise);
      useOrgDesignerStore.setState({ error: "stale" });

      const pending = useOrgDesignerStore.getState().loadSnapshot();
      expect(useOrgDesignerStore.getState().loading).toBe(true);
      expect(useOrgDesignerStore.getState().error).toBeNull();

      const snap = makeSnapshot();
      d.resolve(snap);
      await pending;

      const s = useOrgDesignerStore.getState();
      expect(mockGetSnapshot).toHaveBeenCalledWith(false);
      expect(s.loading).toBe(false);
      expect(s.snapshot).toEqual(snap);
      expect(s.workspaceMode).toBe("published");
      expect(s.error).toBeNull();
    });

    it("requests the draft view when the workspace is in draft mode", async () => {
      useOrgDesignerStore.setState({ workspaceMode: "draft", snapshot: makeBothSnapshot() });
      mockGetSnapshot.mockResolvedValueOnce(makeBothSnapshot({ display_model_id: 2 }));

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(mockGetSnapshot).toHaveBeenCalledWith(true);
      expect(useOrgDesignerStore.getState().workspaceMode).toBe("draft");
    });

    it("forces published mode when no draft model exists", async () => {
      useOrgDesignerStore.setState({ workspaceMode: "draft", snapshot: makeBothSnapshot() });
      mockGetSnapshot.mockResolvedValueOnce(makeSnapshot());

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(useOrgDesignerStore.getState().workspaceMode).toBe("published");
    });

    it("forces draft mode when only a draft exists (nothing published yet)", async () => {
      mockGetSnapshot.mockResolvedValueOnce(
        makeSnapshot({
          active_model_id: null,
          active_model_version: null,
          draft_model_id: 2,
          draft_model_version: 1,
          display_model_id: 2,
        }),
      );

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(useOrgDesignerStore.getState().workspaceMode).toBe("draft");
    });

    it("defaults to published on the first load when both models exist", async () => {
      useOrgDesignerStore.setState({ workspaceMode: "draft" });
      mockGetSnapshot.mockResolvedValueOnce(makeBothSnapshot());

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(useOrgDesignerStore.getState().workspaceMode).toBe("published");
    });

    it("switches to draft when a draft appears after a previous draft-less snapshot", async () => {
      useOrgDesignerStore.setState({ workspaceMode: "published", snapshot: makeSnapshot() });
      mockGetSnapshot.mockResolvedValueOnce(makeBothSnapshot());

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(useOrgDesignerStore.getState().workspaceMode).toBe("draft");
    });

    it("keeps the current mode when both models existed before and still exist", async () => {
      useOrgDesignerStore.setState({ workspaceMode: "published", snapshot: makeBothSnapshot() });
      mockGetSnapshot.mockResolvedValueOnce(makeBothSnapshot({ active_model_version: 4 }));

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(useOrgDesignerStore.getState().workspaceMode).toBe("published");
    });

    it("formats the error, keeps the old snapshot and clears loading on failure", async () => {
      const previous = makeSnapshot();
      useOrgDesignerStore.setState({ snapshot: previous });
      mockGetSnapshot.mockRejectedValueOnce(new Error("Validation failed: model locked"));

      await expect(useOrgDesignerStore.getState().loadSnapshot()).resolves.toBeUndefined();

      const s = useOrgDesignerStore.getState();
      // formatOrgIpcError strips the "Validation failed:" prefix.
      expect(s.error).toBe("model locked");
      expect(s.snapshot).toEqual(previous);
      expect(s.loading).toBe(false);
    });

    it("joins structured IPC validation details into the error", async () => {
      mockGetSnapshot.mockRejectedValueOnce({
        code: "VALIDATION_FAILED",
        message: "Validation failed",
        details: [{ message: "first problem" }, { message: "second problem" }],
      });

      await useOrgDesignerStore.getState().loadSnapshot();

      expect(useOrgDesignerStore.getState().error).toBe("first problem; second problem");
    });
  });

  describe("setWorkspaceMode", () => {
    it("sets the mode, clears selection and reloads with the new preference", async () => {
      useOrgDesignerStore.setState({ snapshot: makeBothSnapshot(), selectedNodeId: 9 });
      const d = deferred<OrgDesignerSnapshot>();
      mockGetSnapshot.mockReturnValueOnce(d.promise);

      useOrgDesignerStore.getState().setWorkspaceMode("draft");

      let s = useOrgDesignerStore.getState();
      expect(s.workspaceMode).toBe("draft");
      expect(s.selectedNodeId).toBeNull();
      expect(s.loading).toBe(true);
      expect(mockGetSnapshot).toHaveBeenCalledWith(true);

      d.resolve(makeBothSnapshot({ display_model_id: 2 }));
      await vi.waitFor(() => expect(useOrgDesignerStore.getState().loading).toBe(false));

      s = useOrgDesignerStore.getState();
      expect(s.workspaceMode).toBe("draft");
      expect(s.snapshot?.display_model_id).toBe(2);
    });

    it("reverts to published when the reload shows no draft", async () => {
      useOrgDesignerStore.setState({ snapshot: makeSnapshot() });
      mockGetSnapshot.mockResolvedValueOnce(makeSnapshot());

      useOrgDesignerStore.getState().setWorkspaceMode("draft");

      await vi.waitFor(() => expect(useOrgDesignerStore.getState().loading).toBe(false));
      expect(mockGetSnapshot).toHaveBeenCalledWith(true);
      expect(useOrgDesignerStore.getState().workspaceMode).toBe("published");
    });
  });

  describe("filters and selection", () => {
    it("setFilterText / setStatusFilter / setTypeFilter update their fields", () => {
      const st = useOrgDesignerStore.getState();
      st.setFilterText("pump");
      st.setStatusFilter("active");
      st.setTypeFilter("SITE");

      let s = useOrgDesignerStore.getState();
      expect(s.filterText).toBe("pump");
      expect(s.statusFilter).toBe("active");
      expect(s.typeFilter).toBe("SITE");

      st.setStatusFilter(null);
      st.setTypeFilter(null);
      st.setFilterText("");
      s = useOrgDesignerStore.getState();
      expect(s.filterText).toBe("");
      expect(s.statusFilter).toBeNull();
      expect(s.typeFilter).toBeNull();
    });

    it("setSelectedNodeId sets and clears the selected node", () => {
      useOrgDesignerStore.getState().setSelectedNodeId(42);
      expect(useOrgDesignerStore.getState().selectedNodeId).toBe(42);

      useOrgDesignerStore.getState().setSelectedNodeId(null);
      expect(useOrgDesignerStore.getState().selectedNodeId).toBeNull();
    });
  });

  describe("impact preview", () => {
    it("opens the drawer immediately, then stores the preview", async () => {
      const d = deferred<OrgImpactPreview>();
      mockPreview.mockReturnValueOnce(d.promise);
      useOrgDesignerStore.setState({ preview: makePreview({ subject_node_id: 99 }) });
      const payload = makePayload();

      const pending = useOrgDesignerStore.getState().openPreview(payload);

      let s = useOrgDesignerStore.getState();
      expect(s.previewOpen).toBe(true);
      expect(s.previewLoading).toBe(true);
      expect(s.preview).toBeNull();
      expect(s.previewPayload).toEqual(payload);
      expect(mockPreview).toHaveBeenCalledWith(payload);

      const preview = makePreview();
      d.resolve(preview);
      await pending;

      s = useOrgDesignerStore.getState();
      expect(s.preview).toEqual(preview);
      expect(s.previewLoading).toBe(false);
      expect(s.previewOpen).toBe(true);
      expect(s.previewPayload).toEqual(payload);
    });

    it("closes the drawer and sets the error when the preview fails", async () => {
      mockPreview.mockRejectedValueOnce(new Error("Node not found"));

      await expect(
        useOrgDesignerStore.getState().openPreview(makePayload()),
      ).resolves.toBeUndefined();

      const s = useOrgDesignerStore.getState();
      expect(s.error).toBe("Node not found");
      expect(s.previewOpen).toBe(false);
      expect(s.previewPayload).toBeNull();
      expect(s.preview).toBeNull();
      expect(s.previewLoading).toBe(false);
    });

    it("closePreview clears preview, payload and open flag", () => {
      useOrgDesignerStore.setState({
        preview: makePreview(),
        previewPayload: makePayload(),
        previewOpen: true,
      });

      useOrgDesignerStore.getState().closePreview();

      const s = useOrgDesignerStore.getState();
      expect(s.preview).toBeNull();
      expect(s.previewPayload).toBeNull();
      expect(s.previewOpen).toBe(false);
    });

    it("a late preview response after the drawer was closed does not refill it", async () => {
      const first = deferred<OrgImpactPreview>();
      mockPreview.mockReturnValueOnce(first.promise);
      const p = useOrgDesignerStore.getState().openPreview(makePayload({ node_id: 5 }));
      useOrgDesignerStore.getState().closePreview();
      first.resolve(makePreview({ subject_node_id: 5 }));
      await p;
      const s = useOrgDesignerStore.getState();
      expect(s.preview).toBeNull();
      expect(s.previewOpen).toBe(false);
      expect(s.previewLoading).toBe(false);
    });

    it("a late response for an earlier preview never overwrites the newer one", async () => {
      const first = deferred<OrgImpactPreview>();
      const second = deferred<OrgImpactPreview>();
      mockPreview.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);

      const pA = useOrgDesignerStore.getState().openPreview(makePayload({ node_id: 5 }));
      const pB = useOrgDesignerStore.getState().openPreview(makePayload({ node_id: 6 }));

      second.resolve(makePreview({ subject_node_id: 6 }));
      await pB;
      first.resolve(makePreview({ subject_node_id: 5 }));
      await pA;

      const s = useOrgDesignerStore.getState();
      expect(s.previewPayload?.node_id).toBe(6);
      expect(s.preview?.subject_node_id).toBe(6);
    });
  });
});
