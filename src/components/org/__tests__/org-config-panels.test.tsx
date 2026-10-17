/**
 * Org designer configuration panels — states and confirmations.
 *
 *  - Hierarchy rules: a failed load is an error with retry (never "no rule"); removing a rule
 *    asks first and only then calls the backend.
 *  - Node types: a failed load is an error with retry; deactivating asks first.
 *  - Audit history: a failed load is an error with retry and does not block publishing.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mockListOrgNodeTypes = vi.fn();
const mockListOrgRelationshipRules = vi.fn();
const mockDeleteOrgRelationshipRule = vi.fn();
const mockCreateOrgRelationshipRule = vi.fn();
const mockGetOrgNodeTypeUsageCount = vi.fn();
const mockDeactivateOrgNodeType = vi.fn();

vi.mock("@/services/org-service", () => ({
  listOrgNodeTypes: (...a: unknown[]) => mockListOrgNodeTypes(...a),
  listOrgRelationshipRules: (...a: unknown[]) => mockListOrgRelationshipRules(...a),
  deleteOrgRelationshipRule: (...a: unknown[]) => mockDeleteOrgRelationshipRule(...a),
  createOrgRelationshipRule: (...a: unknown[]) => mockCreateOrgRelationshipRule(...a),
  getOrgNodeTypeUsageCount: (...a: unknown[]) => mockGetOrgNodeTypeUsageCount(...a),
  deactivateOrgNodeType: (...a: unknown[]) => mockDeactivateOrgNodeType(...a),
  createOrgNodeType: vi.fn(),
  updateOrgNodeType: vi.fn(),
}));

const mockListOrgChangeEvents = vi.fn();
vi.mock("@/services/org-governance-service", () => ({
  validateOrgModelForPublish: vi.fn().mockResolvedValue(null),
  publishOrgModel: vi.fn(),
  listOrgChangeEvents: (...a: unknown[]) => mockListOrgChangeEvents(...a),
  reconcileOrgDraftLineage: vi.fn(),
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

import { AuditTimeline } from "@/components/org/AuditTimeline";
import { NodeTypeManagerPanel } from "@/components/org/NodeTypeManagerPanel";
import { OrgRelationshipRulesPanel } from "@/components/org/OrgRelationshipRulesPanel";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import type { OrgNodeType, OrgRelationshipRule } from "@shared/ipc-types";

const siteType = {
  id: 10,
  structure_model_id: 2,
  code: "SITE",
  label: "Site",
  icon_key: null,
  color: "#123456",
  depth_hint: null,
  can_host_assets: true,
  can_own_work: true,
  can_carry_cost_center: false,
  can_aggregate_kpis: false,
  can_receive_permits: false,
  is_root_type: true,
  is_active: true,
  created_at: "2026-01-01T00:00:00Z",
  updated_at: "2026-01-01T00:00:00Z",
} as unknown as OrgNodeType;

const zoneType = { ...siteType, id: 11, code: "ZONE", label: "Zone", is_root_type: false };

const rule = {
  id: 5,
  structure_model_id: 2,
  parent_type_id: 10,
  child_type_id: 11,
  parent_type_label: "Site",
  child_type_label: "Zone",
  min_children: null,
  max_children: null,
  created_at: "2026-01-01T00:00:00Z",
} as unknown as OrgRelationshipRule;

beforeEach(() => {
  vi.clearAllMocks();
  useOrgGovernanceStore.setState({
    publishValidation: null,
    error: null,
    auditError: null,
    auditEvents: [],
    auditLoading: false,
  });
});

describe("OrgRelationshipRulesPanel", () => {
  it("shows a load failure with retry instead of an empty rule list", async () => {
    mockListOrgNodeTypes.mockRejectedValueOnce({ code: "DB", message: "down" });
    mockListOrgRelationshipRules.mockResolvedValueOnce([]);

    render(
      <OrgRelationshipRulesPanel
        open
        onOpenChange={vi.fn()}
        structureModelId={2}
        onChanged={vi.fn()}
      />,
    );

    expect(await screen.findByText("relationshipRules.loadError")).toBeInTheDocument();
    expect(screen.queryByText("relationshipRules.empty")).not.toBeInTheDocument();

    mockListOrgNodeTypes.mockResolvedValueOnce([siteType, zoneType]);
    mockListOrgRelationshipRules.mockResolvedValueOnce([rule]);
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(await screen.findByText("Zone")).toBeInTheDocument();
  });

  it("asks before removing a rule and removes only after confirmation", async () => {
    mockListOrgNodeTypes.mockResolvedValue([siteType, zoneType]);
    mockListOrgRelationshipRules.mockResolvedValue([rule]);
    mockDeleteOrgRelationshipRule.mockResolvedValueOnce(undefined);
    const onChanged = vi.fn();

    render(
      <OrgRelationshipRulesPanel
        open
        onOpenChange={vi.fn()}
        structureModelId={2}
        onChanged={onChanged}
      />,
    );

    const removeButton = await screen.findByRole("button", {
      name: /relationshipRules\.remove — /,
    });
    fireEvent.click(removeButton);

    expect(await screen.findByText("relationshipRules.removeConfirmTitle")).toBeInTheDocument();
    expect(mockDeleteOrgRelationshipRule).not.toHaveBeenCalled();

    const confirm = screen
      .getAllByRole("button", { name: "relationshipRules.remove" })
      .find((b) => !b.getAttribute("aria-label"));
    expect(confirm).toBeDefined();
    fireEvent.click(confirm as HTMLElement);

    await waitFor(() => expect(mockDeleteOrgRelationshipRule).toHaveBeenCalledWith(5));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });
});

describe("NodeTypeManagerPanel", () => {
  it("shows a load failure with retry instead of 'no type yet'", async () => {
    mockListOrgNodeTypes.mockRejectedValueOnce({ code: "DB", message: "down" });

    render(
      <NodeTypeManagerPanel
        open
        onOpenChange={vi.fn()}
        structureModelId={2}
        onTypesChanged={vi.fn()}
      />,
    );

    expect(await screen.findByText("nodeTypes.loadError")).toBeInTheDocument();
    expect(screen.queryByText("nodeTypes.empty")).not.toBeInTheDocument();
  });

  it("asks before deactivating an unused type", async () => {
    mockListOrgNodeTypes.mockResolvedValue([zoneType]);
    mockGetOrgNodeTypeUsageCount.mockResolvedValue(0);
    mockDeactivateOrgNodeType.mockResolvedValueOnce(undefined);
    const onTypesChanged = vi.fn();

    render(
      <NodeTypeManagerPanel
        open
        onOpenChange={vi.fn()}
        structureModelId={2}
        onTypesChanged={onTypesChanged}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: "nodeTypes.deactivateNamed" }));
    expect(await screen.findByText("nodeTypes.deactivateConfirmTitle")).toBeInTheDocument();
    expect(mockDeactivateOrgNodeType).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "nodeTypes.deactivate" }));
    await waitFor(() => expect(mockDeactivateOrgNodeType).toHaveBeenCalledWith(11));
    await waitFor(() => expect(onTypesChanged).toHaveBeenCalled());
  });
});

describe("AuditTimeline", () => {
  it("shows a failed history load with retry and leaves the publish error empty", async () => {
    mockListOrgChangeEvents.mockRejectedValueOnce({ code: "DB", message: "audit down" });

    render(<AuditTimeline />);

    expect(await screen.findByText("governance.auditLoadError")).toBeInTheDocument();
    expect(screen.queryByText("governance.noAuditEvents")).not.toBeInTheDocument();
    expect(useOrgGovernanceStore.getState().error).toBeNull();

    mockListOrgChangeEvents.mockResolvedValueOnce([]);
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    await waitFor(() =>
      expect(screen.queryByText("governance.auditLoadError")).not.toBeInTheDocument(),
    );
  });
});
