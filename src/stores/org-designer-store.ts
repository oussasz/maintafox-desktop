/**
 * org-designer-store.ts
 *
 * Zustand store for the organization designer workspace.
 * Manages the designer snapshot, filters, selected-node context,
 * and impact-preview drawer state.
 */

import { create } from "zustand";

import { createLatestRequestGuard } from "@/lib/latest-request";
import { getOrgDesignerSnapshot, previewOrgChange } from "@/services/org-designer-service";
import type {
  OrgDesignerSnapshot,
  OrgImpactPreview,
  PreviewOrgChangePayload,
} from "@shared/ipc-types";

export type OrgDesignerWorkspaceMode = "published" | "draft";

const requests = createLatestRequestGuard();

/** Editable “design the draft” session: a draft must exist and the user must have switched to Draft. */
export function isOrgStructureDesignMode(
  snapshot: OrgDesignerSnapshot | null,
  mode: OrgDesignerWorkspaceMode,
): boolean {
  return snapshot?.draft_model_id != null && mode === "draft";
}

/**
 * Id of the published node that carries a unit's operational records (responsibilities, external
 * links, equipment, work). On the published tree that is the node itself; a draft copy points at
 * it through `origin_node_id`; a unit that exists only in the draft has none yet (`null`).
 */
export function operationalNodeIdOf(
  snapshot: OrgDesignerSnapshot | null,
  nodeId: number,
): number | null {
  const row = snapshot?.nodes.find((n) => n.node_id === nodeId);
  if (!snapshot || !row) return nodeId;
  if (row.structure_model_id == null || row.structure_model_id === snapshot.active_model_id) {
    return nodeId;
  }
  return row.origin_node_id ?? null;
}

/**
 * Mode after a snapshot load: no draft → published; only a draft → draft; a draft that has just
 * appeared → draft (the user just created it); otherwise keep the mode the user chose. A first
 * load with both models opens on the published tree.
 */
function resolveWorkspaceMode(
  prev: OrgDesignerSnapshot | null,
  next: OrgDesignerSnapshot,
  current: OrgDesignerWorkspaceMode,
): OrgDesignerWorkspaceMode {
  if (next.draft_model_id == null) return "published";
  if (next.active_model_id == null) return "draft";
  if (prev == null) return "published";
  if (prev.draft_model_id == null) return "draft";
  return current;
}

/**
 * Keep the selected unit when the tree changes model: a published unit becomes its draft copy
 * (`origin_node_id`), a draft copy becomes its published unit. Anything else is dropped.
 */
function carrySelection(
  prev: OrgDesignerSnapshot | null,
  next: OrgDesignerSnapshot,
  selectedNodeId: number | null,
): number | null {
  if (selectedNodeId == null) return null;
  if (prev == null || prev.display_model_id === next.display_model_id) return selectedNodeId;
  const copy = next.nodes.find((n) => n.origin_node_id === selectedNodeId);
  if (copy) return copy.node_id;
  const origin = prev.nodes.find((n) => n.node_id === selectedNodeId)?.origin_node_id;
  if (origin != null && next.nodes.some((n) => n.node_id === origin)) return origin;
  return null;
}

interface OrgDesignerStoreState {
  snapshot: OrgDesignerSnapshot | null;
  /** published = viewing live org; draft = working on a draft (when a draft model exists). */
  workspaceMode: OrgDesignerWorkspaceMode;
  filterText: string;
  statusFilter: string | null;
  typeFilter: string | null;
  selectedNodeId: number | null;
  preview: OrgImpactPreview | null;
  previewPayload: PreviewOrgChangePayload | null;
  /** Human name of the change target (new parent unit, new assignee) for the drawer. */
  previewTargetLabel: string | null;
  previewOpen: boolean;
  loading: boolean;
  previewLoading: boolean;
  /** Failure of the last snapshot load (raw; render with ErrorState / describeAppError). */
  error: unknown;

  loadSnapshot: () => Promise<void>;
  setWorkspaceMode: (value: OrgDesignerWorkspaceMode) => void;
  setFilterText: (value: string) => void;
  setStatusFilter: (value: string | null) => void;
  setTypeFilter: (value: string | null) => void;
  setSelectedNodeId: (nodeId: number | null) => void;
  openPreview: (
    payload: PreviewOrgChangePayload,
    context?: { targetLabel?: string | null },
  ) => Promise<void>;
  closePreview: () => void;
}

export const useOrgDesignerStore = create<OrgDesignerStoreState>()((set, get) => ({
  snapshot: null,
  workspaceMode: "published",
  filterText: "",
  statusFilter: null,
  typeFilter: null,
  selectedNodeId: null,
  preview: null,
  previewPayload: null,
  previewTargetLabel: null,
  previewOpen: false,
  loading: false,
  previewLoading: false,
  error: null,

  loadSnapshot: async () => {
    // Switching published/draft quickly reloads twice: only the latest snapshot may land.
    const isCurrent = requests.begin("snapshot");
    set({ loading: true, error: null });
    try {
      const st = get();
      let snapshot = await getOrgDesignerSnapshot(st.workspaceMode === "draft");
      if (!isCurrent()) return;
      const workspaceMode = resolveWorkspaceMode(st.snapshot, snapshot, st.workspaceMode);
      // A draft that was just created switches the screen to draft mode, but the tree above was
      // loaded for the previous mode. Showing the published tree in draft mode mixes the two
      // models: "+ Enfant" then finds no allowed type under a published parent. Load the tree
      // that belongs to the resolved mode.
      const wantedModelId =
        workspaceMode === "draft" ? snapshot.draft_model_id : snapshot.active_model_id;
      if (wantedModelId != null && snapshot.display_model_id !== wantedModelId) {
        snapshot = await getOrgDesignerSnapshot(workspaceMode === "draft");
        if (!isCurrent()) return;
      }
      set({
        snapshot,
        workspaceMode,
        selectedNodeId: carrySelection(st.snapshot, snapshot, get().selectedNodeId),
        error: null,
        loading: false,
      });
    } catch (err) {
      if (!isCurrent()) return;
      set({ error: err, loading: false });
    }
  },

  setWorkspaceMode: (value) => {
    // The selection is carried to the matching unit of the other tree when it loads.
    set({ workspaceMode: value });
    void get().loadSnapshot();
  },

  setFilterText: (value) => set({ filterText: value }),

  setStatusFilter: (value) => set({ statusFilter: value }),

  setTypeFilter: (value) => set({ typeFilter: value }),

  setSelectedNodeId: (nodeId) => set({ selectedNodeId: nodeId }),

  openPreview: async (payload, context) => {
    // Only the preview of the latest request may fill the drawer; a closed drawer stays empty.
    const isCurrent = requests.begin("preview");
    set({
      previewLoading: true,
      previewOpen: true,
      preview: null,
      previewPayload: payload,
      previewTargetLabel: context?.targetLabel ?? null,
    });
    try {
      const preview = await previewOrgChange(payload);
      if (!isCurrent()) return;
      set({ preview, previewLoading: false });
    } catch {
      if (!isCurrent()) return;
      // The global IPC toast reports the failure; the page keeps its tree.
      set({
        previewOpen: false,
        previewPayload: null,
        previewTargetLabel: null,
        previewLoading: false,
      });
    }
  },

  closePreview: () => {
    requests.invalidate("preview");
    set({
      preview: null,
      previewPayload: null,
      previewTargetLabel: null,
      previewOpen: false,
      previewLoading: false,
    });
  },
}));
