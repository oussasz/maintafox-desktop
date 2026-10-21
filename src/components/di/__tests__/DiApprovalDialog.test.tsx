/**
 * DiApprovalDialog — approve and convert are confirmed in a dialog with an action summary
 * (owner decision 2026-10-14: no password re-entry; RBAC and audit stay on the backend).
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enDi from "@/i18n/locale-data/en/di.json";
import { useAppToastStore } from "@/store/app-toast-store";
import type { InterventionRequest } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { DiApprovalDialog } from "../DiApprovalDialog";

function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = ns === "common" ? lookup(enCommon, key) : lookup(enDi, key);
      if (value == null) throw new Error(`missing ${ns ?? "di"} i18n key: ${key}`);
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en", getFixedT: () => (k: string) => k },
  }),
}));

const navigate = vi.hoisted(() => vi.fn());
vi.mock("react-router-dom", () => ({ useNavigate: () => navigate }));
vi.mock("@/i18n", () => ({ i18n: { language: "en" } }));
vi.mock("@/components/common/LinkedEntityBadge", () => ({ LinkedEntityBadge: () => null }));
vi.mock("@/components/di/di-reference-labels", () => ({
  useDiReferenceLabels: () => ({ originLabel: () => "Operator", symptomLabel: () => "Vibration" }),
  fetchDiSymptomLabel: vi.fn(),
  loadDiOriginLabelMap: vi.fn(),
  resolveDiOriginLabel: vi.fn(),
}));
// Rights at the request's site (`useAccessAt`): every right here; the gating is tested below.
const accessAt = vi.hoisted(() => ({
  nodes: [] as (number | null | undefined)[],
  can: (_permission: string): boolean => true,
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    accessAt.nodes.push(node);
    return { can: (p: string) => accessAt.can(p), loading: false };
  },
}));
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 5, display_name: "Amina", username: "amina" } }),
}));

const conversion = vi.hoisted(() => ({ convertDiToWo: vi.fn() }));
vi.mock("@/services/di-conversion-service", () => conversion);

const review = vi.hoisted(() => ({
  approvalDi: null as InterventionRequest | null,
  closeApproval: vi.fn(),
  openRejection: vi.fn(),
  approve: vi.fn(),
  saving: false,
  error: null,
}));
vi.mock("@/stores/di-review-store", () => ({
  useDiReviewStore: (sel: (s: typeof review) => unknown) => sel(review),
}));
const diStore = vi.hoisted(() => ({ loadDis: vi.fn() }));
vi.mock("@/stores/di-store", () => ({
  useDiStore: (sel: (s: typeof diStore) => unknown) => sel(diStore),
}));

function di(overrides: Partial<InterventionRequest> = {}): InterventionRequest {
  return {
    id: 31,
    code: "DI-0031",
    title: "Pump P-101 vibration",
    description: "Strong vibration on the drive end",
    status: "awaiting_approval",
    row_version: 4,
    reported_urgency: "high",
    origin_type: "operator",
    symptom_code_id: 77,
    submitted_at: "2026-10-14T08:00:00Z",
    submitter_display_name: "Karim",
    asset_code: "P-101",
    asset_label: "Feed pump",
    org_node_id: 12,
    org_node_code: "UTL",
    org_node_label: "Utilities",
    reviewer_note: null,
    converted_to_wo_id: null,
    converted_to_wo_code: null,
    converted_to_wo_title: null,
    ...overrides,
  } as unknown as InterventionRequest;
}

describe("DiApprovalDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    review.saving = false;
    accessAt.nodes = [];
    accessAt.can = () => true;
  });

  it("approves after a confirmation that names the request, without asking for a password", async () => {
    review.approvalDi = di();
    let resolveApprove: (v: unknown) => void = () => {};
    review.approve.mockReturnValue(new Promise((r) => (resolveApprove = r)));

    render(<DiApprovalDialog />);
    fireEvent.click(screen.getByRole("button", { name: enDi.action.approve }));

    const confirm = await screen.findByRole("dialog", { name: enDi.review.confirmApproveTitle });
    expect(confirm).toHaveTextContent("DI-0031 will be approved");
    expect(screen.queryByLabelText(/password|mot de passe/i)).toBeNull();
    expect(review.approve).not.toHaveBeenCalled();

    const submit = within(confirm).getByRole("button", { name: enDi.review.confirmApproveAction });
    fireEvent.click(submit);
    fireEvent.click(submit);
    expect(review.approve).toHaveBeenCalledTimes(1);
    expect(review.approve).toHaveBeenCalledWith({
      di_id: 31,
      actor_id: 5,
      expected_row_version: 4,
      notes: null,
    });

    resolveApprove({ approvedRowVersion: 5 });
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: enDi.review.confirmApproveTitle }),
      ).not.toBeInTheDocument(),
    );
  });

  it("cancelling the confirmation does nothing", async () => {
    review.approvalDi = di();
    render(<DiApprovalDialog />);
    fireEvent.click(screen.getByRole("button", { name: enDi.action.approve }));
    const confirm = await screen.findByRole("dialog", { name: enDi.review.confirmApproveTitle });
    fireEvent.click(within(confirm).getByRole("button", { name: enCommon.ds.confirm.cancel }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: enDi.review.confirmApproveTitle }),
      ).not.toBeInTheDocument(),
    );
    expect(review.approve).not.toHaveBeenCalled();
  });

  it("converts an approved request after confirmation, says what it decided about the permit and opens the new work order", async () => {
    review.approvalDi = di({ status: "approved" } as unknown as Partial<InterventionRequest>);
    conversion.convertDiToWo.mockResolvedValue({
      wo_id: 88,
      wo_code: "OT-0088",
      di: di({ status: "converted_to_work_order" } as unknown as Partial<InterventionRequest>),
      permit_flag: {
        requires_permit: true,
        reason_code: "PERMIT_RULE_MATCHED",
        rule_code: "PUMP_HOT",
        permit_type_code: "HOT_WORK",
        safety_flag: false,
        unreadable_rule_codes: [],
      },
    });
    useAppToastStore.setState({ items: [] });

    render(<DiApprovalDialog />);
    fireEvent.click(screen.getByRole("button", { name: enDi.review.convertToWo }));
    const confirm = await screen.findByRole("dialog", { name: enDi.review.confirmConvertTitle });
    expect(confirm).toHaveTextContent("A work order will be created from DI-0031");
    fireEvent.click(
      within(confirm).getByRole("button", { name: enDi.review.confirmConvertAction }),
    );

    await waitFor(() => expect(navigate).toHaveBeenCalledWith("/work-orders?openWo=88"));
    expect(conversion.convertDiToWo).toHaveBeenCalledWith({ diId: 31, expectedRowVersion: 4 });
    expect(useAppToastStore.getState().items).toEqual([
      expect.objectContaining({
        title: "Permit required for OT-0088",
        description:
          "The rule PUMP_HOT calls for a HOT_WORK permit: the work order will not start without an active permit.",
      }),
    ]);
  });

  it("offers the conversion from the rights at the request's site, work-order create included", () => {
    review.approvalDi = di({ status: "approved" } as unknown as Partial<InterventionRequest>);
    // di.convert at this site, but not the right to create the work order there.
    accessAt.can = (p) => p === P.DI_CONVERT;
    const { unmount } = render(<DiApprovalDialog />);
    expect(accessAt.nodes).toContain(12);
    expect(screen.queryByRole("button", { name: enDi.review.convertToWo })).toBeNull();
    unmount();

    accessAt.can = (p) => p === P.DI_CONVERT || p === P.OT_CREATE;
    render(<DiApprovalDialog />);
    expect(screen.getByRole("button", { name: enDi.review.convertToWo })).toBeInTheDocument();
  });
});
