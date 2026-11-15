/**
 * Who may carry out a work order (L5-08, CMMS Advisor 2026-11-05): someone who holds the edit
 * right at its site but is neither its assignee, a member of its team, its planner nor an
 * approver there sees its execution actions greyed, with who it is assigned to — the row menu's
 * "Terminer" / "Annuler", and the close-out details and close. The standing comes from the
 * backend (`get_wo_execution_standing`); while it is not known nothing is greyed on that basis.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enCommon from "@/i18n/en/common.json";
import enErrors from "@/i18n/en/errors.json";
import frErrors from "@/i18n/fr/errors.json";
import enAdmin from "@/i18n/locale-data/en/admin.json";
import enOt from "@/i18n/locale-data/en/ot.json";
import enPermits from "@/i18n/locale-data/en/permits.json";
import type * as CloseoutService from "@/services/wo-closeout-service";
import type * as WoService from "@/services/wo-service";
import type { WoExecutionStanding, WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { WoCloseOutPanel } from "../WoCloseOutPanel";
import { WoContextMenu } from "../WoContextMenu";

function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

const CATALOGS: Record<string, unknown> = {
  ot: enOt,
  permits: enPermits,
  common: enCommon,
  admin: enAdmin,
  errors: enErrors,
};

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const value = lookup(CATALOGS[ns ?? "ot"], key);
      if (value == null)
        return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: "en" },
  }),
}));

const standing = vi.hoisted(() => ({
  get: vi.fn<(woId: number) => Promise<WoExecutionStanding>>(),
}));
const closeout = vi.hoisted(() => ({
  saveFailureDetail: vi.fn(),
  saveVerification: vi.fn(),
  closeWo: vi.fn(),
  reopenWo: vi.fn(),
  updateWoRca: vi.fn(),
  updateServiceCost: vi.fn(),
  getWoAnalyticsSnapshot: vi.fn(),
  getWoForCloseOut: vi.fn(),
}));
vi.mock("@/services/wo-closeout-service", async (importOriginal) => {
  const actual = await importOriginal<typeof CloseoutService>();
  return { ...actual, ...closeout };
});
vi.mock("@/services/wo-service", async (importOriginal) => ({
  ...(await importOriginal<typeof WoService>()),
  evaluateWoCompletionGates: vi.fn(async () => []),
  getWoExecutionStanding: standing.get,
}));
vi.mock("@/services/settings-service", () => ({ getSetting: vi.fn(async () => null) }));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));
vi.mock("@/components/common/LinkedEntityBadge", () => ({ LinkedEntityBadge: () => null }));
vi.mock("../use-wo-people", () => ({
  useWoPeopleOptions: () => ({ options: [], loading: false, failed: false }),
  mergeWoPersonOptions: (base: unknown[]) => base,
}));
const rights = vi.hoisted(() => ({ held: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => rights.held.has(p) }),
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: () => ({ loading: false, failed: false, can: (p: string) => rights.held.has(p) }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 5 } }) }));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: vi.fn(), StepUpDialogElement: null }),
}));
vi.mock("@/stores/wo-store", () => ({
  useWoStore: (sel: (s: { refreshActiveWo: () => Promise<void> }) => unknown) =>
    sel({ refreshActiveWo: vi.fn(async () => {}) }),
}));

const COLLEAGUE: WoExecutionStanding = {
  actor: null,
  refusal: {
    code: "GATE_WO_EXECUTION_ASSIGNED_PERSON_TEAM",
    params: { assignee: "Karim Benali", team: "North crew" },
  },
  own_draft: false,
};
const ASSIGNEE: WoExecutionStanding = { actor: "assignee", refusal: null, own_draft: false };
const REASON = enErrors.gateError.GATE_WO_EXECUTION_ASSIGNED_PERSON_TEAM.replace(
  "{{assignee}}",
  "Karim Benali",
).replace("{{team}}", "North crew");

function workOrder(overrides: Partial<WorkOrder> = {}): WorkOrder {
  return {
    id: 42,
    code: "OT-0042",
    status_code: "in_progress",
    row_version: 7,
    entity_id: 5,
    production_impact_id: null,
    ...overrides,
  } as unknown as WorkOrder;
}

describe("the execution reasons are written in both languages", () => {
  it("has a French and an English text naming the assignee or the team", () => {
    for (const code of [
      "GATE_WO_EXECUTION_ASSIGNED_PERSON",
      "GATE_WO_EXECUTION_ASSIGNED_TEAM",
      "GATE_WO_EXECUTION_ASSIGNED_PERSON_TEAM",
      "GATE_WO_EXECUTION_UNASSIGNED",
    ] as const) {
      expect(frErrors.gateError[code]).toMatch(/heures/);
      expect(enErrors.gateError[code]).toMatch(/hours/);
    }
    expect(frErrors.gateError.GATE_WO_EXECUTION_ASSIGNED_PERSON).toContain("{{assignee}}");
    expect(frErrors.gateError.GATE_WO_EXECUTION_ASSIGNED_TEAM).toContain("{{team}}");
  });
});

describe("row menu: Terminer and Annuler by who carries out the work order (L5-08)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    rights.held = new Set<string>([P.OT_VIEW, P.OT_EDIT]);
  });

  function openMenu(wo: WorkOrder) {
    const onComplete = vi.fn();
    const onCancel = vi.fn();
    render(
      <WoContextMenu wo={wo} onViewDetail={vi.fn()} onComplete={onComplete} onCancel={onCancel}>
        <button type="button">actions</button>
      </WoContextMenu>,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "actions" }), { key: "Enter" });
    return { onComplete, onCancel };
  }

  const item = (label: string) =>
    screen.getAllByRole("menuitem").find((el) => el.textContent?.startsWith(label));

  it("greys them with the assignee and team named for a colleague", async () => {
    standing.get.mockResolvedValue(COLLEAGUE);
    const { onComplete, onCancel } = openMenu(workOrder());
    await waitFor(() => expect(screen.getAllByText(REASON)).toHaveLength(2));
    for (const label of [enOt.contextMenu.complete, enOt.contextMenu.cancel]) {
      const el = item(label);
      expect(el).toHaveAttribute("data-disabled");
      expect(el).toHaveAttribute("title", REASON);
      if (el) fireEvent.click(el);
    }
    expect(onComplete).not.toHaveBeenCalled();
    expect(onCancel).not.toHaveBeenCalled();
    expect(standing.get).toHaveBeenCalledWith(42);
  });

  it("greys Démarrer on a ready work order for a colleague", async () => {
    standing.get.mockResolvedValue(COLLEAGUE);
    const onStart = vi.fn();
    render(
      <WoContextMenu
        wo={workOrder({ status_code: "ready" })}
        onViewDetail={vi.fn()}
        onStart={onStart}
      >
        <button type="button">actions</button>
      </WoContextMenu>,
    );
    fireEvent.keyDown(screen.getByRole("button", { name: "actions" }), { key: "Enter" });
    await waitFor(() => expect(item(enOt.contextMenu.start)).toHaveAttribute("data-disabled"));
    expect(item(enOt.contextMenu.start)).toHaveAttribute("title", REASON);
  });

  it("lets the creator cancel their own draft, the rest staying greyed", async () => {
    standing.get.mockResolvedValue({ ...COLLEAGUE, own_draft: true });
    openMenu(workOrder({ status_code: "draft" }));
    await waitFor(() => expect(standing.get).toHaveBeenCalledWith(42));
    expect(await screen.findByText(enOt.contextMenu.cancel)).toBeInTheDocument();
    expect(item(enOt.contextMenu.cancel)).not.toHaveAttribute("data-disabled");
    expect(screen.queryByText(REASON)).toBeNull();
  });

  it("offers them to the assignee", async () => {
    standing.get.mockResolvedValue(ASSIGNEE);
    openMenu(workOrder());
    await waitFor(() => expect(standing.get).toHaveBeenCalledWith(42));
    expect(await screen.findByText(enOt.contextMenu.complete)).toBeInTheDocument();
    expect(item(enOt.contextMenu.complete)).not.toHaveAttribute("data-disabled");
    expect(item(enOt.contextMenu.cancel)).not.toHaveAttribute("data-disabled");
    expect(screen.queryByText(REASON)).toBeNull();
  });

  it("greys nothing on that basis while the standing cannot be read (the server decides)", async () => {
    standing.get.mockRejectedValue(new Error("offline"));
    openMenu(workOrder());
    await waitFor(() => expect(standing.get).toHaveBeenCalled());
    expect(await screen.findByText(enOt.contextMenu.cancel)).toBeInTheDocument();
    expect(item(enOt.contextMenu.cancel)).not.toHaveAttribute("data-disabled");
    expect(screen.queryByText(REASON)).toBeNull();
  });
});

describe("close-out: details and close by who carries out the work order (L5-08)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    rights.held = new Set<string>([P.OT_VIEW, P.OT_EDIT, P.OT_CLOSE]);
    closeout.getWoAnalyticsSnapshot.mockResolvedValue(null);
  });

  const saveButton = () => screen.getByRole("button", { name: enOt.closeout.saveDetails });
  const closeButton = () => screen.getByRole("button", { name: enOt.closeout.closeWo });

  function show(wo: WorkOrder) {
    closeout.getWoForCloseOut.mockResolvedValue({ wo, failure_detail: null });
    render(<WoCloseOutPanel wo={wo} canEdit onClosed={vi.fn()} />);
  }

  it("greys saving the details and closing for a colleague, naming who it is assigned to", async () => {
    standing.get.mockResolvedValue(COLLEAGUE);
    show(workOrder({ status_code: "completed" }));
    await waitFor(() => expect(saveButton()).toHaveAttribute("title", REASON));
    expect(saveButton()).toBeDisabled();
    expect(screen.getByText(REASON, { selector: "#wo-closeout-details-blocker" })).toBeVisible();
    expect(closeButton()).toBeDisabled();
    expect(closeButton()).toHaveAttribute("title", REASON);
    expect(screen.getByTestId("wo-close-blocked")).toHaveTextContent(REASON);
    // The full closure form is not shown to them.
    expect(screen.queryByText(enOt.closeout.closureWarning)).toBeNull();
  });

  it("names the missing close right first: the rule never stands in for it", async () => {
    rights.held = new Set<string>([P.OT_VIEW, P.OT_EDIT]);
    standing.get.mockResolvedValue(COLLEAGUE);
    show(workOrder({ status_code: "completed" }));
    const right = enOt.closeout.blocked.rightRequired.replace(
      "{{right}}",
      enAdmin.permissions.items.ot_close.label,
    );
    await waitFor(() => expect(saveButton()).toHaveAttribute("title", REASON));
    expect(closeButton()).toHaveAttribute("title", right);
  });

  it("offers saving and closing to the assignee", async () => {
    standing.get.mockResolvedValue(ASSIGNEE);
    show(workOrder({ status_code: "completed" }));
    await waitFor(() => expect(standing.get).toHaveBeenCalledWith(42));
    await waitFor(() => expect(saveButton()).toBeEnabled());
    expect(closeButton()).toBeEnabled();
    expect(screen.getByText(enOt.closeout.closureWarning)).toBeInTheDocument();
    expect(screen.queryByText(REASON)).toBeNull();
  });
});
