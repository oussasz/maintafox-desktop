/**
 * impact-preview-drawer.test.tsx
 *
 * Sprint S3 — Component smoke tests for ImpactPreviewDrawer.
 *
 * TC1 - Blockers present → confirm button disabled
 * TC2 - Warnings only → warning list shown, confirm enabled after acknowledgement
 * TC3 - Closing drawer clears preview state in the store
 */

import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";

// ── Service mocks (store imports the service) ─────────────────────────────────

vi.mock("@/services/org-designer-service", () => ({
  getOrgDesignerSnapshot: vi.fn().mockResolvedValue({
    active_model_id: null,
    active_model_version: null,
    nodes: [],
  }),
  searchOrgDesignerNodes: vi.fn().mockResolvedValue([]),
  previewOrgChange: vi.fn().mockResolvedValue(null),
}));

// i18n pass-through
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

// ── Imports after mocks ───────────────────────────────────────────────────────

import { ImpactPreviewDrawer } from "@/components/org/ImpactPreviewDrawer";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import type { OrgImpactPreview } from "@shared/ipc-types";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const blockerPreview: OrgImpactPreview = {
  action: "DeactivateNode",
  subject_node_id: 1,
  affected_node_count: 5,
  descendant_count: 4,
  active_responsibility_count: 3,
  active_binding_count: 1,
  blockers: [
    "Node has 4 active descendants that must be deactivated first",
    "Node has 3 active responsibilities that must be ended first",
  ],
  warnings: [],
  dependencies: [
    {
      domain: "assets",
      status: "unavailable",
      count: null,
      note: "Module 6.3 not yet implemented",
    },
    {
      domain: "open_work",
      status: "unavailable",
      count: null,
      note: "Modules 6.4/6.5 not yet implemented",
    },
  ],
};

const warningOnlyPreview: OrgImpactPreview = {
  action: "MoveNode",
  subject_node_id: 2,
  affected_node_count: 3,
  descendant_count: 2,
  active_responsibility_count: 1,
  active_binding_count: 2,
  blockers: [],
  warnings: [
    "Subtree contains 1 active responsibility that may need updating",
    "Subtree contains 2 active external bindings",
  ],
  dependencies: [
    {
      domain: "assets",
      status: "unavailable",
      count: null,
      note: "Module 6.3 not yet implemented",
    },
  ],
};

const cleanPreview: OrgImpactPreview = {
  action: "ReassignResponsibility",
  subject_node_id: 3,
  affected_node_count: 1,
  descendant_count: 0,
  active_responsibility_count: 1,
  active_binding_count: 0,
  blockers: [],
  warnings: [],
  dependencies: [],
};

// ── Store reset helper ────────────────────────────────────────────────────────

function resetStore() {
  useOrgDesignerStore.setState({
    snapshot: null,
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

describe("ImpactPreviewDrawer — Sprint S3 smoke tests", () => {
  beforeEach(() => {
    resetStore();
    vi.clearAllMocks();
  });

  // ── TC1 — Blockers disable confirm ─────────────────────────────────────

  describe("TC1 - preview with blockers disables confirm", () => {
    it("renders blocker messages and disables the confirm button", () => {
      useOrgDesignerStore.setState({
        preview: blockerPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      // Blocker section heading
      expect(screen.getByText("preview.blockers")).toBeInTheDocument();

      // Both blocker messages rendered
      expect(
        screen.getByText("Node has 4 active descendants that must be deactivated first"),
      ).toBeInTheDocument();
      expect(
        screen.getByText("Node has 3 active responsibilities that must be ended first"),
      ).toBeInTheDocument();

      // Confirm button should show "blocked" label and be disabled
      const confirmBtn = screen.getByRole("button", { name: /preview\.blocked/i });
      expect(confirmBtn).toBeDisabled();
    });

    it("does not show acknowledgement checkbox when blockers exist", () => {
      useOrgDesignerStore.setState({
        preview: blockerPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      expect(screen.queryByText("preview.acknowledgeWarnings")).not.toBeInTheDocument();
    });
  });

  // ── TC2 — Warnings show list, confirm enabled after acknowledgement ────

  describe("TC2 - preview with warnings shows warning list and enabled confirm", () => {
    it("renders warning messages", () => {
      useOrgDesignerStore.setState({
        preview: warningOnlyPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      // Warning section heading
      expect(screen.getByText("preview.warnings")).toBeInTheDocument();

      // Warning messages
      expect(
        screen.getByText("Subtree contains 1 active responsibility that may need updating"),
      ).toBeInTheDocument();
      expect(screen.getByText("Subtree contains 2 active external bindings")).toBeInTheDocument();
    });

    it("confirm is disabled until warnings are acknowledged", () => {
      useOrgDesignerStore.setState({
        preview: warningOnlyPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      // Before acknowledgement: confirm disabled
      const confirmBtn = screen.getByRole("button", { name: /preview\.confirm/i });
      expect(confirmBtn).toBeDisabled();

      // Acknowledge warnings
      const checkbox = screen.getByRole("checkbox");
      fireEvent.click(checkbox);

      // After acknowledgement: confirm enabled
      expect(confirmBtn).toBeEnabled();
    });

    it("confirm is immediately enabled when no blockers and no warnings", () => {
      useOrgDesignerStore.setState({
        preview: cleanPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      const confirmBtn = screen.getByRole("button", { name: /preview\.confirm/i });
      expect(confirmBtn).toBeEnabled();
    });
  });

  // ── TC3 — Closing drawer clears preview state ──────────────────────────

  describe("TC3 - closing the drawer clears preview state in the store", () => {
    it("cancel button clears preview and previewOpen", () => {
      useOrgDesignerStore.setState({
        preview: blockerPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      // Click cancel
      const cancelBtn = screen.getByRole("button", { name: /preview\.cancel/i });
      fireEvent.click(cancelBtn);

      // Store should be cleared
      const state = useOrgDesignerStore.getState();
      expect(state.preview).toBeNull();
      expect(state.previewOpen).toBe(false);
    });

    it("old blockers do not leak into a subsequent preview", () => {
      // First preview: blockers
      useOrgDesignerStore.setState({
        preview: blockerPreview,
        previewOpen: true,
        previewLoading: false,
      });

      const { unmount } = render(<ImpactPreviewDrawer />);
      expect(
        screen.getByText("Node has 4 active descendants that must be deactivated first"),
      ).toBeInTheDocument();

      // Close
      fireEvent.click(screen.getByRole("button", { name: /preview\.cancel/i }));
      unmount();

      // Second preview: clean (no blockers, no warnings)
      useOrgDesignerStore.setState({
        preview: cleanPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      // Old blocker text must not appear
      expect(
        screen.queryByText("Node has 4 active descendants that must be deactivated first"),
      ).not.toBeInTheDocument();

      // No blocker section at all
      expect(screen.queryByText("preview.blockers")).not.toBeInTheDocument();

      // Confirm should be enabled
      const confirmBtn = screen.getByRole("button", { name: /preview\.confirm/i });
      expect(confirmBtn).toBeEnabled();
    });

    it("warning acknowledgement resets between drawer sessions", () => {
      // First: open with warnings, acknowledge, close
      useOrgDesignerStore.setState({
        preview: warningOnlyPreview,
        previewOpen: true,
        previewLoading: false,
      });

      const { unmount } = render(<ImpactPreviewDrawer />);
      fireEvent.click(screen.getByRole("checkbox"));
      expect(screen.getByRole("button", { name: /preview\.confirm/i })).toBeEnabled();

      // Close via cancel
      fireEvent.click(screen.getByRole("button", { name: /preview\.cancel/i }));
      unmount();

      // Reopen with same warnings
      useOrgDesignerStore.setState({
        preview: warningOnlyPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      // Checkbox should be unchecked again, confirm disabled
      const checkbox = screen.getByRole("checkbox");
      expect(checkbox).not.toBeChecked();
      expect(screen.getByRole("button", { name: /preview\.confirm/i })).toBeDisabled();
    });
  });

  // ── Dependency placeholders ────────────────────────────────────────────

  describe("Dependency placeholders", () => {
    it("renders dependency domains with their status badges", () => {
      useOrgDesignerStore.setState({
        preview: blockerPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      expect(screen.getByText("preview.dependencies")).toBeInTheDocument();
      expect(screen.getByText("assets")).toBeInTheDocument();
      expect(screen.getByText("open_work")).toBeInTheDocument();
      expect(screen.getByText("Module 6.3 not yet implemented")).toBeInTheDocument();
      expect(screen.getAllByText("unavailable").length).toBeGreaterThanOrEqual(2);
    });
  });

  // ── Impact summary counters ────────────────────────────────────────────

  describe("Impact summary counters", () => {
    it("displays all four summary counters", () => {
      useOrgDesignerStore.setState({
        preview: blockerPreview,
        previewOpen: true,
        previewLoading: false,
      });

      render(<ImpactPreviewDrawer />);

      expect(screen.getByText("preview.impactSummary")).toBeInTheDocument();
      expect(screen.getByText("5")).toBeInTheDocument(); // affected_node_count
      expect(screen.getByText("4")).toBeInTheDocument(); // descendant_count
      expect(screen.getByText("3")).toBeInTheDocument(); // active_responsibility_count
      expect(screen.getByText("1")).toBeInTheDocument(); // active_binding_count
    });
  });
});
