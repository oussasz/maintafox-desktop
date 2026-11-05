/**
 * A permit flag set by a plant rule: named with its permit type, cleared only with the
 * permit-issue right at the work order's site and a reason of 10 characters or more, after a
 * confirmation. Every permit type the rules require is listed with its state, each lifted on its
 * own (L4-10b).
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RequiredPermitView, WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && Object.keys(opts).length > 0 ? `${key} ${JSON.stringify(opts)}` : key,
    i18n: { language: "en" },
  }),
}));

// The plant-wide rights (the rights at no node, `useAccessAt(0)`) and the rights at a work order's
// site: clearing a rule's permit type is read at the work order's site (lead decision D-a).
const granted = vi.hoisted(() => ({
  rights: new Set<string>(),
  /** The rights at a work order's site (any node other than 0). */
  siteRights: new Set<string>(),
  nodes: [] as (number | null | undefined)[],
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    granted.nodes.push(node);
    return {
      loading: false,
      can: (permission: string) =>
        node === 0 ? granted.rights.has(permission) : granted.siteRights.has(permission),
    };
  },
}));

const svc = vi.hoisted(() => ({
  PERMIT_REQUIREMENT_EDITABLE_STATUSES: new Set(["draft", "planning", "ready"]),
  clearWoPermitRuleFlag: vi.fn(),
}));
vi.mock("@/services/wo-service", () => svc);

import {
  WoPermitRuleNote,
  permitFromSafetyRequestOnly,
  permitRuleFlagActive,
} from "../WoPermitRuleNote";

function wo(overrides: Partial<WorkOrder> = {}): WorkOrder {
  return {
    id: 12,
    code: "OT-0012",
    status_code: "planning",
    row_version: 3,
    requires_permit: true,
    permit_rule_code: "PUMP_HOT",
    permit_rule_permit_type: "HOT_WORK",
    permit_rule_cleared_by_id: null,
    permit_rule_cleared_at: null,
    permit_rule_clear_reason: null,
    ...overrides,
  } as WorkOrder;
}

beforeEach(() => {
  vi.clearAllMocks();
  granted.rights = new Set();
  granted.siteRights = new Set();
  granted.nodes = [];
});

describe("WoPermitRuleNote", () => {
  it("names the rule and the permit type it calls for", () => {
    render(<WoPermitRuleNote wo={wo()} />);
    expect(screen.getByTestId("wo-permit-rule")).toHaveTextContent(
      'permitRule.requiredBy {"rule":"PUMP_HOT","permitType":"HOT_WORK"}',
    );
    expect(permitRuleFlagActive(wo())).toBe(true);
  });

  it("offers no clearing without the permit-issue right", () => {
    render(<WoPermitRuleNote wo={wo()} />);
    expect(screen.queryByRole("button", { name: "permitRule.clear" })).toBeNull();
  });

  it("clears only after a confirmation with a reason of 10 characters or more", async () => {
    granted.siteRights = new Set([P.PTW_ISSUE]);
    const saved = wo({ requires_permit: false, permit_rule_cleared_at: "2026-10-18T10:00:00Z" });
    svc.clearWoPermitRuleFlag.mockResolvedValue(saved);
    const onCleared = vi.fn();
    render(
      <WoPermitRuleNote wo={wo({ entity_id: 7 } as Partial<WorkOrder>)} onCleared={onCleared} />,
    );
    // The right is read at the work order's site (lead decision D-a).
    expect(granted.nodes).toContain(7);

    fireEvent.click(screen.getByRole("button", { name: "permitRule.clear" }));
    const confirm = screen.getByRole("button", { name: "permitRule.clearConfirm" });
    fireEvent.change(screen.getByLabelText("permitRule.reason"), {
      target: { value: "too short" },
    });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("permitRule.reason"), {
      target: { value: "  Cold work only, no welding  " },
    });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(svc.clearWoPermitRuleFlag).toHaveBeenCalledWith({
        wo_id: 12,
        expected_row_version: 3,
        reason: "Cold work only, no welding",
      }),
    );
    await waitFor(() => expect(onCleared).toHaveBeenCalledWith(saved));
  });

  it("is not clearable once execution has started", () => {
    granted.siteRights = new Set([P.PTW_ISSUE]);
    render(<WoPermitRuleNote wo={wo({ status_code: "in_progress" })} />);
    expect(screen.queryByRole("button", { name: "permitRule.clear" })).toBeNull();
  });

  it("shows when and why a cleared flag was cleared, and nothing without a rule", () => {
    const { rerender } = render(
      <WoPermitRuleNote
        wo={wo({
          requires_permit: false,
          permit_rule_cleared_at: "2026-10-18T10:00:00Z",
          permit_rule_clear_reason: "Cold work only",
        })}
      />,
    );
    expect(screen.getByTestId("wo-permit-rule")).toHaveTextContent("permitRule.cleared");
    expect(screen.getByTestId("wo-permit-rule")).toHaveTextContent("Cold work only");
    rerender(<WoPermitRuleNote wo={wo({ permit_rule_code: null })} />);
    expect(screen.queryByTestId("wo-permit-rule")).toBeNull();
  });
});

// Every permit type the plant's rules require (CMMS Advisor 2026-11-04, L4-10b): listed with its
// rule and state, each lifted on its own with the right at the work order's site.
describe("WoPermitRuleNote — required permits", () => {
  const permits: RequiredPermitView[] = [
    {
      permit_type_code: "loto",
      permit_type_name: "Consignation",
      rule_code: "CRITICITE_A",
      primary: true,
      cleared_at: null,
      cleared_by_name: null,
      clear_reason: null,
      live_permit_code: "PTW-0004",
      live_permit_status: "active",
      state: "live",
    },
    {
      permit_type_code: "hot_work",
      permit_type_name: "Travail à chaud",
      rule_code: "SOUDURE",
      primary: false,
      cleared_at: null,
      cleared_by_name: null,
      clear_reason: null,
      live_permit_code: null,
      live_permit_status: null,
      state: "missing",
    },
    {
      permit_type_code: "height",
      permit_type_name: "Travail en hauteur",
      rule_code: "HAUTEUR",
      primary: false,
      cleared_at: "2026-11-04T08:00:00Z",
      cleared_by_name: "Issuer",
      clear_reason: "Nacelle non utilisée",
      live_permit_code: null,
      live_permit_status: null,
      state: "cleared",
    },
  ];
  const multi = (overrides: Partial<WorkOrder> = {}) =>
    wo({
      permit_rule_code: "CRITICITE_A",
      permit_rule_permit_type: "loto",
      entity_id: 7,
      required_permits: permits,
      ...overrides,
    } as Partial<WorkOrder>);

  it("lists each required permit with its rule, the primary one and its state", () => {
    render(<WoPermitRuleNote wo={multi()} />);
    const loto = screen.getByTestId("wo-required-permit-loto");
    expect(loto).toHaveAttribute("data-state", "live");
    expect(loto).toHaveTextContent("Consignation");
    expect(loto).toHaveTextContent('permitRule.listRule {"rule":"CRITICITE_A"}');
    expect(loto).toHaveTextContent("permitRule.listPrimary");
    expect(loto).toHaveTextContent('permitRule.listLive {"permit":"PTW-0004","status":"active"}');
    const hot = screen.getByTestId("wo-required-permit-hot_work");
    expect(hot).toHaveAttribute("data-state", "missing");
    expect(hot).toHaveTextContent("permitRule.listMissing");
    expect(hot).not.toHaveTextContent("permitRule.listPrimary");
    const height = screen.getByTestId("wo-required-permit-height");
    expect(height).toHaveAttribute("data-state", "cleared");
    expect(height).toHaveTextContent("Nacelle non utilisée");
    expect(height).toHaveTextContent("Issuer");
  });

  it("stays protected while one type is not cleared, even with the primary rule cleared", () => {
    const primaryCleared = multi({
      requires_permit: true,
      permit_rule_cleared_at: "2026-11-04T08:00:00Z",
    });
    expect(permitRuleFlagActive(primaryCleared)).toBe(true);
    const allCleared = multi({
      requires_permit: false,
      permit_rule_cleared_at: "2026-11-04T08:00:00Z",
      required_permits: permits.map((p) => ({ ...p, state: "cleared" as const })),
    });
    expect(permitRuleFlagActive(allCleared)).toBe(false);
    expect(
      permitFromSafetyRequestOnly(multi({ permit_rule_code: null, permit_from_safety_di: true })),
    ).toBe(false);
  });

  it("offers no lifting without the permit-issue right at the work order's site", () => {
    granted.rights = new Set();
    granted.siteRights = new Set();
    render(<WoPermitRuleNote wo={multi()} />);
    expect(screen.queryAllByRole("button", { name: /permitRule.listClear/ })).toHaveLength(0);
  });

  it("lifts one type at a time, naming it, after a confirmation with a reason", async () => {
    granted.siteRights = new Set([P.PTW_ISSUE]);
    const saved = multi();
    svc.clearWoPermitRuleFlag.mockResolvedValue(saved);
    const onCleared = vi.fn();
    render(<WoPermitRuleNote wo={multi()} onCleared={onCleared} />);
    expect(granted.nodes).toContain(7);
    // A button per type not cleared; none on the cleared one.
    expect(screen.getAllByRole("button", { name: /permitRule.listClear/ })).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "permitRule.listClear Travail à chaud" }));
    const confirm = screen.getByRole("button", { name: "permitRule.clearConfirm" });
    fireEvent.change(screen.getByLabelText("permitRule.reason"), { target: { value: "court" } });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("permitRule.reason"), {
      target: { value: "Meulage à froid seulement" },
    });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(svc.clearWoPermitRuleFlag).toHaveBeenCalledWith({
        wo_id: 12,
        expected_row_version: 3,
        reason: "Meulage à froid seulement",
        permit_type_code: "hot_work",
      }),
    );
    await waitFor(() => expect(onCleared).toHaveBeenCalledWith(saved));
  });

  it("is not liftable once execution has started", () => {
    granted.siteRights = new Set([P.PTW_ISSUE]);
    render(<WoPermitRuleNote wo={multi({ status_code: "in_progress" })} />);
    expect(screen.queryAllByRole("button", { name: /permitRule.listClear/ })).toHaveLength(0);
  });
});

// A permit requirement carried from a request flagged "sécurité" (CMMS Advisor decision D1): locked
// like a rule flag, cleared with the permit-issue right at the work order's site and a reason.
describe("WoPermitRuleNote — safety request", () => {
  const safety = (overrides: Partial<WorkOrder> = {}) =>
    wo({
      permit_rule_code: null,
      permit_rule_permit_type: null,
      permit_from_safety_di: true,
      entity_id: 7,
      ...overrides,
    } as Partial<WorkOrder>);

  it("is a protected requirement, named by its origin", () => {
    render(<WoPermitRuleNote wo={safety()} />);
    expect(permitRuleFlagActive(safety())).toBe(true);
    expect(permitFromSafetyRequestOnly(safety())).toBe(true);
    expect(screen.getByTestId("wo-permit-rule")).toHaveTextContent(
      "permitRequirement.fromSafetyDi",
    );
    expect(permitRuleFlagActive(safety({ permit_from_safety_di: false }))).toBe(false);
    expect(permitRuleFlagActive(safety({ requires_permit: false }))).toBe(false);
  });

  it("offers no clearing without the permit-issue right at its site", () => {
    granted.siteRights = new Set();
    render(<WoPermitRuleNote wo={safety()} />);
    expect(screen.queryByRole("button", { name: "permitRule.clear" })).toBeNull();
  });

  it("clears with the right at the work order's site and a reason of 10 characters or more", async () => {
    granted.siteRights = new Set([P.PTW_ISSUE]);
    const saved = safety({
      requires_permit: false,
      permit_rule_cleared_at: "2026-10-22T10:00:00Z",
    });
    svc.clearWoPermitRuleFlag.mockResolvedValue(saved);
    const onCleared = vi.fn();
    render(<WoPermitRuleNote wo={safety()} onCleared={onCleared} />);
    expect(granted.nodes).toContain(7);

    fireEvent.click(screen.getByRole("button", { name: "permitRule.clear" }));
    const confirm = screen.getByRole("button", { name: "permitRule.clearConfirm" });
    fireEvent.change(screen.getByLabelText("permitRule.reason"), { target: { value: "court" } });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("permitRule.reason"), {
      target: { value: "Inspection visuelle seulement" },
    });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(svc.clearWoPermitRuleFlag).toHaveBeenCalledWith({
        wo_id: 12,
        expected_row_version: 3,
        reason: "Inspection visuelle seulement",
      }),
    );
    await waitFor(() => expect(onCleared).toHaveBeenCalledWith(saved));
  });

  it("shows when and why it was cleared", () => {
    render(
      <WoPermitRuleNote
        wo={safety({
          requires_permit: false,
          permit_rule_cleared_at: "2026-10-22T10:00:00Z",
          permit_rule_clear_reason: "Inspection visuelle seulement",
        })}
      />,
    );
    expect(screen.getByTestId("wo-permit-rule")).toHaveTextContent("permitSafety.cleared");
    expect(screen.getByTestId("wo-permit-rule")).toHaveTextContent("Inspection visuelle seulement");
  });
});
