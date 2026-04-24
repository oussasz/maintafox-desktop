/**
 * org-governance-supervisor.test.tsx
 *
 * Supervisor verification tests for Sprint S3 — Governance UI.
 *
 * V1 - Validation result with blockers disables the publish button
 * V2 - Successful publish refreshes snapshot/governance state
 * V3 - Audit timeline renders rows from listOrgChangeEvents
 */

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

const mockValidateOrgModelForPublish = vi.fn();
const mockPublishOrgModel = vi.fn();
const mockListOrgChangeEvents = vi.fn();

vi.mock("@/services/org-governance-service", () => ({
  validateOrgModelForPublish: (...args: unknown[]) => mockValidateOrgModelForPublish(...args),
  publishOrgModel: (...args: unknown[]) => mockPublishOrgModel(...args),
  listOrgChangeEvents: (...args: unknown[]) => mockListOrgChangeEvents(...args),
}));

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
      if (opts && "count" in opts) return `${key}::count=${opts["count"]}`;
      if (opts && "version" in opts) return `${key}::version=${opts["version"]}`;
      if (opts && "remapCount" in opts) return `${key}::remapCount=${opts["remapCount"]}`;
      return key;
    },
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

// ── Import after mocks ───────────────────────────────────────────────────────

import { AuditTimeline } from "@/components/org/AuditTimeline";
import { OrganizationDesignerPage } from "@/pages/admin/OrganizationDesignerPage";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import type {
  OrgChangeEvent,
  OrgDesignerSnapshot,
  OrgPublishValidationResult,
} from "@shared/ipc-types";

// ── Fixtures ──────────────────────────────────────────────────────────────────

const activeSnapshot: OrgDesignerSnapshot = {
  active_model_id: 1,
  active_model_version: 3,
  draft_model_id: 2,
  draft_model_version: 1,
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
      child_count: 0,
      active_responsibility_count: 0,
      active_binding_count: 0,
    },
  ],
};

const blockedValidation: OrgPublishValidationResult = {
  model_id: 2,
  can_publish: false,
  issue_count: 2,
  blocking_count: 2,
  issues: [
    {
      code: "MISSING_TYPE_CODE",
      severity: "error",
      message: "Node type WORKSHOP has no mapping",
      related_id: 5,
    },
    {
      code: "PARENT_CHILD_DRIFT",
      severity: "error",
      message: "DEPT→TEAM rule removed",
      related_id: 6,
    },
  ],
  remap_count: 0,
};

const passValidation: OrgPublishValidationResult = {
  model_id: 2,
  can_publish: true,
  issue_count: 0,
  blocking_count: 0,
  issues: [],
  remap_count: 3,
};

const sampleAuditEvents: OrgChangeEvent[] = [
  {
    id: 3,
    entity_kind: "structure_model",
    entity_id: 2,
    change_type: "publish_model",
    before_json: null,
    after_json: "{}",
    preview_summary_json: null,
    changed_by_id: 1,
    changed_at: "2026-04-06T12:00:00Z",
    requires_step_up: true,
    apply_result: "applied",
  },
  {
    id: 2,
    entity_kind: "org_node",
    entity_id: 42,
    change_type: "move_node",
    before_json: '{"parent_id":1}',
    after_json: '{"parent_id":2}',
    preview_summary_json: null,
    changed_by_id: 1,
    changed_at: "2026-04-06T11:30:00Z",
    requires_step_up: true,
    apply_result: "applied",
  },
  {
    id: 1,
    entity_kind: "org_node",
    entity_id: 10,
    change_type: "update_metadata",
    before_json: '{"name":"Old"}',
    after_json: '{"name":"New"}',
    preview_summary_json: null,
    changed_by_id: 1,
    changed_at: "2026-04-06T11:00:00Z",
    requires_step_up: false,
    apply_result: "applied",
  },
];

// ── Store reset helpers ───────────────────────────────────────────────────────

function resetStores() {
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
  useOrgGovernanceStore.setState({
    publishValidation: null,
    validationLoading: false,
    auditEvents: [],
    auditLoading: false,
    error: null,
  });
}

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("Supervisor Verification — Sprint S3 Governance UI", () => {
  beforeEach(() => {
    resetStores();
    vi.clearAllMocks();
  });

  // ── V1 — Validation blockers disable the publish button ───────────────────

  describe("V1 - Publish-readiness banner with blockers", () => {
    it("disables the publish button when validation has blockers", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValue(activeSnapshot);
      mockValidateOrgModelForPublish.mockResolvedValue(blockedValidation);
      mockListOrgChangeEvents.mockResolvedValue([]);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("designer.title")).toBeInTheDocument();
      });
      await act(async () => {
        useOrgDesignerStore.setState({ workspaceMode: "draft" });
      });

      await waitFor(() => {
        expect(screen.getByTestId("publish-blockers-banner")).toBeInTheDocument();
      });

      // The publish button must be disabled
      const publishBtn = screen.getByTestId("publish-button");
      expect(publishBtn).toBeDisabled();

      // The blocking issues list must be rendered
      const issuesList = screen.getByTestId("blocking-issues-list");
      expect(issuesList).toBeInTheDocument();
      expect(issuesList.querySelectorAll("li").length).toBeGreaterThanOrEqual(2);

      // Issue codes must be visible
      expect(screen.getByText("MISSING_TYPE_CODE")).toBeInTheDocument();
      expect(screen.getByText("PARENT_CHILD_DRIFT")).toBeInTheDocument();
    });

    it("enables the publish button when validation passes", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValue(activeSnapshot);
      mockValidateOrgModelForPublish.mockResolvedValue(passValidation);
      mockListOrgChangeEvents.mockResolvedValue([]);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("designer.title")).toBeInTheDocument();
      });
      await act(async () => {
        useOrgDesignerStore.setState({ workspaceMode: "draft" });
      });

      await waitFor(() => {
        expect(screen.getByTestId("publish-ready-banner")).toBeInTheDocument();
      });

      const publishBtn = screen.getByTestId("publish-button");
      expect(publishBtn).not.toBeDisabled();
    });
  });

  // ── V2 — Successful publish refreshes snapshot and governance ─────────────

  describe("V2 - Publish success refresh", () => {
    it("calls publishOrgModel and refreshes snapshot after success", async () => {
      mockGetOrgDesignerSnapshot.mockResolvedValue(activeSnapshot);
      mockValidateOrgModelForPublish.mockResolvedValue(passValidation);
      mockPublishOrgModel.mockResolvedValue(passValidation);
      mockListOrgChangeEvents.mockResolvedValue([]);

      render(<OrganizationDesignerPage />);

      await waitFor(() => {
        expect(screen.getByText("designer.title")).toBeInTheDocument();
      });
      await act(async () => {
        useOrgDesignerStore.setState({ workspaceMode: "draft" });
      });

      // Wait for ready state
      await waitFor(() => {
        expect(screen.getByTestId("publish-button")).not.toBeDisabled();
      });

      // Click publish
      fireEvent.click(screen.getByTestId("publish-button"));

      // publishOrgModel must have been called
      await waitFor(() => {
        expect(mockPublishOrgModel).toHaveBeenCalledWith(2);
      });

      // Snapshot should be reloaded (getOrgDesignerSnapshot called again)
      // The initial call + the refresh call
      expect(mockGetOrgDesignerSnapshot.mock.calls.length).toBeGreaterThanOrEqual(2);
    });
  });

  // ── V3 — Audit timeline renders rows ──────────────────────────────────────

  describe("V3 - Audit timeline", () => {
    it("renders audit event rows from listOrgChangeEvents", async () => {
      mockListOrgChangeEvents.mockResolvedValue(sampleAuditEvents);

      render(<AuditTimeline />);

      // Audit timeline container must be present
      await waitFor(() => {
        expect(screen.getByTestId("audit-timeline")).toBeInTheDocument();
      });

      // Three audit event rows must render
      const rows = screen.getAllByTestId("audit-event-row");
      expect(rows).toHaveLength(3);

      // Events must contain the expected change types
      expect(screen.getByText("publish_model")).toBeInTheDocument();
      expect(screen.getByText("move_node")).toBeInTheDocument();
      expect(screen.getByText("update_metadata")).toBeInTheDocument();
    });
  });
});
