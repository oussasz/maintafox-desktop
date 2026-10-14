/**
 * move-node-and-tree.test.tsx
 *
 * - MoveNodeDialog offers only parents the hierarchy rules allow (never the unit itself, its
 *   sub-units or its current parent) and opens the impact preview with the action name and
 *   target the backend expects (`MoveNode` + `new_parent_id`).
 * - OrganizationTreePanel is keyboard usable (one Tab stop, arrows, collapse/expand, Enter).
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockPreviewOrgChange = vi.fn();
const mockListRules = vi.fn();

vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: vi.fn(),
  searchOrgDesignerNodes: vi.fn().mockResolvedValue([]),
  previewOrgChange: (...args: unknown[]) => mockPreviewOrgChange(...args),
}));

vi.mock("@/services/org-service", () => ({
  listOrgRelationshipRules: (...args: unknown[]) => mockListRules(...args),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

import { MoveNodeDialog } from "@/components/org/MoveNodeDialog";
import { OrganizationTreePanel } from "@/components/org/OrganizationTreePanel";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerNodeRow, OrgDesignerSnapshot } from "@shared/ipc-types";

const SITE = 10;
const WORKSHOP = 20;

function row(overrides: Partial<OrgDesignerNodeRow>): OrgDesignerNodeRow {
  return {
    node_id: 1,
    parent_id: null,
    ancestor_path: "/1/",
    depth: 0,
    code: "SITE-A",
    name: "Site Alpha",
    status: "active",
    row_version: 1,
    node_type_id: SITE,
    node_type_code: "SITE",
    node_type_label: "Site",
    can_host_assets: true,
    can_own_work: true,
    can_carry_cost_center: true,
    can_aggregate_kpis: false,
    can_receive_permits: false,
    child_count: 0,
    active_responsibility_count: 0,
    active_binding_count: 0,
    structure_model_id: 2,
    origin_node_id: null,
    ...overrides,
  };
}

// Site A ─ WS-1 ─ WS-1a ; Site A ─ WS-2 ; Site B
const snapshot: OrgDesignerSnapshot = {
  active_model_id: 1,
  active_model_version: 1,
  draft_model_id: 2,
  draft_model_version: 2,
  display_model_id: 2,
  nodes: [
    row({ node_id: 1, child_count: 2 }),
    row({
      node_id: 2,
      parent_id: 1,
      ancestor_path: "/1/2/",
      depth: 1,
      code: "WS-1",
      name: "Workshop 1",
      node_type_id: WORKSHOP,
      node_type_label: "Workshop",
      child_count: 1,
      row_version: 5,
    }),
    row({
      node_id: 3,
      parent_id: 2,
      ancestor_path: "/1/2/3/",
      depth: 2,
      code: "WS-1A",
      name: "Workshop 1a",
      node_type_id: WORKSHOP,
      node_type_label: "Workshop",
    }),
    row({
      node_id: 4,
      parent_id: 1,
      ancestor_path: "/1/4/",
      depth: 1,
      code: "WS-2",
      name: "Workshop 2",
      node_type_id: WORKSHOP,
      node_type_label: "Workshop",
    }),
    row({ node_id: 5, ancestor_path: "/5/", code: "SITE-B", name: "Site Beta" }),
  ],
};

const rules = [
  { id: 1, structure_model_id: 2, parent_type_id: SITE, child_type_id: WORKSHOP },
  { id: 2, structure_model_id: 2, parent_type_id: WORKSHOP, child_type_id: WORKSHOP },
].map((r) => ({ ...r, min_children: null, max_children: null, created_at: "2026-01-01" }));

function resetStore() {
  useOrgDesignerStore.setState({
    snapshot,
    workspaceMode: "draft",
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
  });
}

describe("MoveNodeDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetStore();
    mockListRules.mockResolvedValue(rules);
    mockPreviewOrgChange.mockResolvedValue({
      action: "MoveNode",
      subject_node_id: 2,
      affected_node_count: 2,
      descendant_count: 1,
      active_responsibility_count: 0,
      active_binding_count: 0,
      blockers: [],
      warnings: [],
      dependencies: [],
    });
  });

  it("lists only allowed parents and previews the move with the chosen target", async () => {
    const workshop1 = snapshot.nodes[1] as OrgDesignerNodeRow;
    render(<MoveNodeDialog open onOpenChange={vi.fn()} node={workshop1} />);

    await waitFor(() => expect(mockListRules).toHaveBeenCalledWith(2));
    const options = await screen.findAllByRole("radio");
    const names = options.map((o) => o.textContent ?? "");
    // Allowed: Workshop 2 (WORKSHOP → WORKSHOP) and Site Beta (SITE → WORKSHOP) only —
    // never the unit itself, its sub-unit (Workshop 1a) or its current parent (Site Alpha).
    expect(names).toHaveLength(2);
    expect(names.some((n) => n.startsWith("Workshop 2"))).toBe(true);
    expect(names.some((n) => n.startsWith("Site Beta"))).toBe(true);

    const review = screen.getByRole("button", { name: "move.review" });
    expect(review).toBeDisabled();

    fireEvent.click(
      options.find((o) => (o.textContent ?? "").includes("Site Beta")) as HTMLElement,
    );
    fireEvent.click(review);

    await waitFor(() =>
      expect(mockPreviewOrgChange).toHaveBeenCalledWith({
        action: "MoveNode",
        node_id: 2,
        new_parent_id: 5,
      }),
    );
    expect(useOrgDesignerStore.getState().previewTargetLabel).toBe("Site Beta");
  });

  it("explains when the rules leave no possible parent", async () => {
    mockListRules.mockResolvedValue([]);
    render(
      <MoveNodeDialog open onOpenChange={vi.fn()} node={snapshot.nodes[1] as OrgDesignerNodeRow} />,
    );

    expect(await screen.findByText("move.noCandidates")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "move.review" })).toBeDisabled();
  });
});

describe("OrganizationTreePanel keyboard", () => {
  beforeEach(() => {
    resetStore();
  });

  it("has one Tab stop and moves with the arrow keys", () => {
    render(<OrganizationTreePanel />);
    const items = screen.getAllByRole("treeitem");
    expect(items).toHaveLength(5);
    expect(items.filter((i) => i.getAttribute("tabindex") === "0")).toHaveLength(1);

    const first = items[0] as HTMLElement;
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(document.activeElement).toBe(items[1]);
    fireEvent.keyDown(items[1] as HTMLElement, { key: "End" });
    expect(document.activeElement).toBe(items[4]);
    fireEvent.keyDown(items[4] as HTMLElement, { key: "Home" });
    expect(document.activeElement).toBe(items[0]);
  });

  it("collapses and expands a branch with the arrow keys and selects with Enter", () => {
    render(<OrganizationTreePanel />);
    const site = screen.getAllByRole("treeitem")[0] as HTMLElement;
    expect(site).toHaveAttribute("aria-expanded", "true");
    expect(site).toHaveAttribute("aria-level", "1");

    site.focus();
    fireEvent.keyDown(site, { key: "ArrowLeft" });
    expect(site).toHaveAttribute("aria-expanded", "false");
    // Sub-units of Site Alpha are hidden; Site Beta stays.
    expect(screen.getAllByRole("treeitem")).toHaveLength(2);

    fireEvent.keyDown(site, { key: "ArrowRight" });
    expect(screen.getAllByRole("treeitem")).toHaveLength(5);
    fireEvent.keyDown(site, { key: "ArrowRight" });
    expect(document.activeElement?.textContent).toContain("Workshop 1");

    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "Enter" });
    expect(useOrgDesignerStore.getState().selectedNodeId).toBe(2);

    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "ArrowLeft" });
    // Workshop 1 is expanded, so ← collapses it first.
    expect(screen.getAllByRole("treeitem")).toHaveLength(4);
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(screen.getAllByRole("treeitem")[0]);
  });
});
