/**
 * org-designer-supervisor.test.tsx
 *
 * Supervisor verification tests for Sprint S2 — Organization Designer Workspace UI.
 *
 * V1 - Empty-state rendering (no active model → banner, no crash)
 * V2 - Bilingual labels (French/English key pass-through)
 * V3 - Tree interaction (indentation, search, selection → inspector update)
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi, beforeEach } from "vitest";

// ── Service mocks ─────────────────────────────────────────────────────────────

const mockGetOrgDesignerSnapshot = vi.fn();
const mockSearchOrgDesignerNodes = vi.fn();
const mockPreviewOrgChange = vi.fn();

vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: (...args: unknown[]) => mockGetOrgDesignerSnapshot(...args),
  searchOrgDesignerNodes: (...args: unknown[]) => mockSearchOrgDesignerNodes(...args),
  previewOrgChange: (...args: unknown[]) => mockPreviewOrgChange(...args),
}));

// Mock org-node-service (used by NodeInspectorPanel via org-node-store)
const mockListOrgTree = vi.fn().mockResolvedValue([]);
const mockGetOrgNode = vi.fn().mockResolvedValue(null);
const mockListOrgNodeResponsibilities = vi.fn().mockResolvedValue([]);
const mockListOrgEntityBindings = vi.fn().mockResolvedValue([]);

vi.mock("@/services/org-node-service", () => ({
  listOrgTree: (...args: unknown[]) => mockListOrgTree(...args),
  getOrgNode: (...args: unknown[]) => mockGetOrgNode(...args),
  listOrgNodeResponsibilities: (...args: unknown[]) => mockListOrgNodeResponsibilities(...args),
  listOrgEntityBindings: (...args: unknown[]) => mockListOrgEntityBindings(...args),
}));

// i18n pass-through: returns the key as text
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (opts && "version" in opts) return `${key}::version=${opts["version"]}`;
      return key;
    },
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

// ── Import after mocks (hoisted by vitest) ────────────────────────────────────

import { OrganizationDesignerPage } from "@/pages/admin/OrganizationDesignerPage";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgDesignerSnapshot } from "@shared/ipc-types";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const emptySnapshot: OrgDesignerSnapshot = {
  active_model_id: null,
  active_model_version: null,
  draft_model_id: null,
  draft_model_version: null,
  nodes: [],
};

const threeNodeSnapshot: OrgDesignerSnapshot = {
  active_model_id: 1,
  active_model_version: 3,
  draft_model_id: null,
  draft_model_version: null,
  nodes: [
    {
      node_id: 1,
      parent_id: null,
      ancestor_path: "/1",
      depth: 0,
      code: "SITE-A",
      name: "Site Alpha",
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
      child_count: 1,
      active_responsibility_count: 2,
      active_binding_count: 0,
    },
    {
      node_id: 2,
      parent_id: 1,
      ancestor_path: "/1/2",
      depth: 1,
      code: "DEPT-M",
      name: "Maintenance Dept",
      status: "active",
      row_version: 1,
      node_type_id: 20,
      node_type_code: "DEPT",
      node_type_label: "Department",
      can_host_assets: false,
      can_own_work: true,
      can_carry_cost_center: true,
      can_aggregate_kpis: false,
      can_receive_permits: false,
      child_count: 1,
      active_responsibility_count: 0,
      active_binding_count: 1,
    },
    {
      node_id: 3,
      parent_id: 2,
      ancestor_path: "/1/2/3",
      depth: 2,
      code: "TEAM-E",
      name: "Electrical Team",
      status: "active",
      row_version: 1,
      node_type_id: 30,
      node_type_code: "TEAM",
      node_type_label: "Team",
      can_host_assets: false,
      can_own_work: true,
      can_carry_cost_center: false,
      can_aggregate_kpis: false,
      can_receive_permits: false,
      child_count: 0,
      active_responsibility_count: 1,
      active_binding_count: 0,
    },
  ],
};

// ── Store reset helper ────────────────────────────────────────────────────────

function resetDesignerStore() {
  useOrgDesignerStore.setState({
    snapshot: null,
    workspaceMode: "published",
    filterText: "",
    statusFilter: null,
    typeFilter: null,
    selectedNodeId: null,
    preview: null,
    previewOpen: false,
    loading: false,
    previewLoading: false,
    error: null,
  });
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("Supervisor Verification — Sprint S2 Org Designer UI", () => {
  beforeEach(() => {
    resetDesignerStore();
    vi.clearAllMocks();
  });

  // ── V1 — Empty-state rendering ────────────────────────────────────────────

  describe("V1 - Empty-state rendering", () => {
    it("renders a governed empty state when no structure model exists", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(emptySnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("lifecycle.noModelYet")).toBeInTheDocument();
      });

      expect(screen.getByText("designer.title")).toBeInTheDocument();
    });

    it("does NOT render the three-pane workspace when no model exists", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(emptySnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("lifecycle.noModelYet")).toBeInTheDocument();
      });

      // Filters sidebar should not appear
      expect(screen.queryByText("designer.filters")).not.toBeInTheDocument();
      // Treegrid should not appear
      expect(screen.queryByRole("treegrid")).not.toBeInTheDocument();
    });
  });

  // ── V2 — Bilingual labels ────────────────────────────────────────────────

  describe("V2 - Bilingual labels", () => {
    it("page title, filters, panel headings, and preview action keys all pass through i18n", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        // Page title
        expect(screen.getByText("designer.title")).toBeInTheDocument();
      });

      // Model version badge with interpolation
      expect(screen.getByText("designer.modelVersion::version=3")).toBeInTheDocument();

      // Filter headings
      expect(screen.getByText("designer.filters")).toBeInTheDocument();
      expect(screen.getByText("designer.statusFilter")).toBeInTheDocument();
      expect(screen.getByText("designer.typeFilter")).toBeInTheDocument();

      // Model summary section
      expect(screen.getByText("designer.modelSummary")).toBeInTheDocument();
      expect(screen.getByText("designer.totalNodes")).toBeInTheDocument();
      expect(screen.getByText("designer.nodeTypesCount")).toBeInTheDocument();

      // Refresh button
      expect(screen.getByText("designer.refresh")).toBeInTheDocument();
    });

    it("tree panel search placeholder goes through i18n", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        const searchInput = screen.getByPlaceholderText("designer.searchPlaceholder");
        expect(searchInput).toBeInTheDocument();
      });
    });

    it("capability badges render translated keys", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        // Site Alpha has can_host_assets + can_own_work + can_carry_cost_center + can_aggregate_kpis
        expect(screen.getAllByText("capabilities.asset").length).toBeGreaterThanOrEqual(1);
        expect(screen.getAllByText("capabilities.work").length).toBeGreaterThanOrEqual(1);
        expect(screen.getAllByText("capabilities.cost").length).toBeGreaterThanOrEqual(1);
        expect(screen.getAllByText("capabilities.kpi").length).toBeGreaterThanOrEqual(1);
      });
    });
  });

  // ── V3 — Tree interaction ─────────────────────────────────────────────────

  describe("V3 - Tree interaction", () => {
    it("renders three-level tree with correct depth indentation", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("Site Alpha")).toBeInTheDocument();
      });

      const rows = screen.getAllByRole("row");
      expect(rows).toHaveLength(3);

      // Verify depth-based indentation via inline style
      // depth 0 → 0*20+12 = 12px, depth 1 → 1*20+12 = 32px, depth 2 → 2*20+12 = 52px
      expect(rows[0]).toHaveStyle({ paddingLeft: "12px" });
      expect(rows[1]).toHaveStyle({ paddingLeft: "32px" });
      expect(rows[2]).toHaveStyle({ paddingLeft: "52px" });
    });

    it("shows all three node names and codes", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("Site Alpha")).toBeInTheDocument();
      });

      expect(screen.getByText("SITE-A")).toBeInTheDocument();
      expect(screen.getByText("Maintenance Dept")).toBeInTheDocument();
      expect(screen.getByText("DEPT-M")).toBeInTheDocument();
      expect(screen.getByText("Electrical Team")).toBeInTheDocument();
      expect(screen.getByText("TEAM-E")).toBeInTheDocument();
    });

    it("filters rows by search text (code match)", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("Site Alpha")).toBeInTheDocument();
      });

      const searchInput = screen.getByPlaceholderText("designer.searchPlaceholder");
      fireEvent.change(searchInput, { target: { value: "TEAM" } });

      // Only the Electrical Team row should remain
      expect(screen.queryByText("Site Alpha")).not.toBeInTheDocument();
      expect(screen.queryByText("Maintenance Dept")).not.toBeInTheDocument();
      expect(screen.getByText("Electrical Team")).toBeInTheDocument();
    });

    it("filters rows by search text (name match)", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("Site Alpha")).toBeInTheDocument();
      });

      const searchInput = screen.getByPlaceholderText("designer.searchPlaceholder");
      fireEvent.change(searchInput, { target: { value: "Electrical" } });

      expect(screen.queryByText("Site Alpha")).not.toBeInTheDocument();
      expect(screen.getByText("Electrical Team")).toBeInTheDocument();
    });

    it("selects a row on click and updates the inspector", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);
      mockGetOrgNode.mockResolvedValue({
        id: 2,
        sync_id: "abc",
        code: "DEPT-M",
        name: "Maintenance Dept",
        node_type_id: 20,
        parent_id: 1,
        ancestor_path: "/1/2",
        depth: 1,
        description: null,
        cost_center_code: null,
        external_reference: null,
        status: "active",
        effective_from: null,
        effective_to: null,
        erp_reference: null,
        notes: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        deleted_at: null,
        row_version: 1,
        origin_machine_id: null,
        last_synced_checkpoint: null,
      });
      mockListOrgNodeResponsibilities.mockResolvedValue([]);
      mockListOrgEntityBindings.mockResolvedValue([]);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("Maintenance Dept")).toBeInTheDocument();
      });

      // Before click: inspector shows no-selection message
      expect(screen.getByText("designer.inspector.noSelection")).toBeInTheDocument();

      // Click the second row (Maintenance Dept)
      const rows = screen.getAllByRole("row");
      const secondRow = rows[1] as HTMLElement;
      fireEvent.click(secondRow);

      // After click: the inspector should update
      await waitFor(() => {
        // The inspector header shows the node code (appears in tree + inspector)
        const deptCodes = screen.getAllByText("DEPT-M");
        expect(deptCodes.length).toBeGreaterThanOrEqual(2); // tree row + inspector header
        // The tab labels should appear
        expect(screen.getByText("designer.inspector.details")).toBeInTheDocument();
        expect(screen.getByText("designer.inspector.responsibilities")).toBeInTheDocument();
        expect(screen.getByText("designer.inspector.bindings")).toBeInTheDocument();
        expect(screen.getByText("designer.inspector.actions")).toBeInTheDocument();
      });

      // The selected row should have aria-selected=true
      expect(secondRow).toHaveAttribute("aria-selected", "true");
    });

    it("deselects row when clicking the same row again", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValueOnce(threeNodeSnapshot);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("Site Alpha")).toBeInTheDocument();
      });

      const rows = screen.getAllByRole("row");
      const firstRow = rows[0] as HTMLElement;

      // Select
      fireEvent.click(firstRow);
      expect(firstRow).toHaveAttribute("aria-selected", "true");

      // Deselect
      fireEvent.click(firstRow);

      await waitFor(() => {
        expect(firstRow).toHaveAttribute("aria-selected", "false");
      });

      // Inspector goes back to no-selection
      expect(screen.getByText("designer.inspector.noSelection")).toBeInTheDocument();
    });
  });
});
