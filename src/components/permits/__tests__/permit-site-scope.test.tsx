/**
 * Site-scoped roles on the permit screens: the list says how many permits it leaves out and why
 * (FR and EN), a permit recorded without a site is flagged "to review", the permit's buttons
 * follow the rights held at the permit's site (not a right held on another site), a permit
 * without a site is answered with the plant-wide rights only (node 0), the hand-over form says
 * why nobody can receive, and the create dialog reads the issue right at the equipment's site.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enErrors from "@/i18n/en/errors.json";
import enPermits from "@/i18n/locale-data/en/permits.json";
import frPermits from "@/i18n/locale-data/fr/permits.json";
import type { WorkPermitRecord } from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

const lang = vi.hoisted(() => ({ current: "en" as "en" | "fr" }));

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
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const permits = lang.current === "fr" ? frPermits : enPermits;
      const count = typeof opts?.["count"] === "number" ? opts["count"] : undefined;
      const plural =
        count === undefined
          ? undefined
          : lookup(
              permits,
              `${key}_${count === 1 || (lang.current === "fr" && count === 0) ? "one" : "other"}`,
            );
      const value = key.startsWith("errors:")
        ? lookup(enErrors, key.slice("errors:".length))
        : (plural ?? lookup(permits, key));
      if (value == null) {
        // Shared UI chrome (dialog close, confirm) reads other namespaces; permit keys must exist.
        if (/^(siteScope|handover|create|detail|transition|kpi|actions)\./.test(key))
          throw new Error(`missing ${lang.current} permits i18n key: ${key}`);
        return typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key;
      }
      return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
    },
    i18n: { language: lang.current, resolvedLanguage: lang.current },
  }),
}));

vi.mock("@/lib/app-error", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  appErrorReason: () => "refused",
}));
vi.mock("@/components/permits/loto-card-print", () => ({ printLotoCard: vi.fn() }));

// Rights per org node, as `get_my_access_at` answers them (node 0: the plant-wide rights).
// `can` answers the node's rights even while `loading` (the real hook answers false then, see
// `use-access-at.test.ts`): a loading test here proves the screen's own loading gate, not the hook's.
const rightsAt = vi.hoisted(() => ({
  byNode: new Map<number, string[]>(),
  asked: [] as (number | null | undefined)[],
  loading: false,
}));
vi.mock("@/hooks/use-access-at", () => ({
  useAccessAt: (node: number | null | undefined) => {
    rightsAt.asked.push(node);
    const granted = node == null ? [] : (rightsAt.byNode.get(node) ?? []);
    return {
      loading: rightsAt.loading,
      can: (p: string) => granted.includes(p),
    };
  },
}));
// The "held anywhere" set must not be what these screens read for a permit's buttons.
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => {
    throw new Error("permit buttons must use the rights at the permit's site");
  },
}));
vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({ info: { user_id: 10, username: "u", display_name: "U" } }),
}));

const gate = vi.hoisted(() => ({ permit: null as unknown }));
const svc = vi.hoisted(() => ({
  getPermitForWorkOrder: vi.fn(),
  listPermitIsolations: vi.fn(),
  listPermitIsolationOptions: vi.fn(),
  listPermitSuspensions: vi.fn(),
  listPermitHandoverLogs: vi.fn(),
  listPermitHandoverRoles: vi.fn(),
  listPermitHandoverReceivers: vi.fn(),
  appendPermitHandoverLog: vi.fn(),
  createWorkPermit: vi.fn(),
}));
vi.mock("@/services/permit-service", () => svc);

vi.mock("@/components/assets/AssetPicker", () => ({
  AssetPicker: ({ onChange }: { onChange: (a: unknown) => void }) => (
    <>
      <button type="button" onClick={() => onChange({ id: 1, org_node_id: 3, code: "A" })}>
        pick-site-3
      </button>
      <button type="button" onClick={() => onChange({ id: 2, org_node_id: 4, code: "B" })}>
        pick-site-4
      </button>
    </>
  ),
}));

import { PermitCreateDialog } from "../PermitCreateDialog";
import { PermitDetailPanel } from "../PermitDetailPanel";
import { PermitHandoverForm } from "../PermitHandoverForm";
import {
  PermitHiddenNotice,
  PermitNoSiteBadge,
  permitAccessNode,
  permitWithoutSite,
} from "../PermitSiteScope";
import { WoPermitGateCard } from "../WoPermitGateCard";

function permit(overrides: Partial<WorkPermitRecord> = {}): WorkPermitRecord {
  return {
    id: 41,
    entity_sync_id: "p41",
    code: "PTW-2026-000041",
    linked_work_order_id: null,
    permit_type_id: 1,
    asset_id: 900,
    entity_id: 3,
    status: "active",
    requested_at: "2026-10-10T08:00:00Z",
    issued_at: "2026-10-10T08:30:00Z",
    activated_at: "2026-10-10T09:30:00Z",
    expires_at: "2099-01-01T00:00:00Z",
    closed_at: null,
    handed_back_at: null,
    row_version: 5,
    permit_type_code: "loto",
    permit_type_name: "LOTO",
    asset_code: "PMP-101",
    asset_label: "Feed pump",
    linked_work_order_code: null,
    permit_type_requires_energy_isolation: false,
    permit_type_requires_three_person_removal: false,
    ...overrides,
  };
}

beforeEach(() => {
  lang.current = "en";
  vi.clearAllMocks();
  rightsAt.byNode = new Map();
  rightsAt.asked = [];
  rightsAt.loading = false;
  gate.permit = null;
  svc.listPermitIsolations.mockResolvedValue([]);
  svc.listPermitIsolationOptions.mockResolvedValue({ energy_types: [], isolation_methods: [] });
  svc.listPermitSuspensions.mockResolvedValue([]);
  svc.listPermitHandoverLogs.mockResolvedValue([]);
  svc.listPermitHandoverRoles.mockResolvedValue([
    { code: "MAINT", label: "Maintenance", review_required: false },
  ]);
});

describe("hidden permits notice", () => {
  it("says how many permits are left out and why, in English", () => {
    render(<PermitHiddenNotice outsideScope={3} withoutSite={1} />);
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("3 permits are not shown: outside your sites.");
    expect(notice).toHaveTextContent("1 permit is not shown: it has no site.");
    expect(notice).toHaveTextContent("Ask a plant-wide user to see them.");
  });

  it("says it in French", () => {
    lang.current = "fr";
    render(<PermitHiddenNotice outsideScope={2} withoutSite={0} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "2 permis ne sont pas affichés : hors de vos sites.",
    );
    expect(screen.getByRole("status")).not.toHaveTextContent("n’ont pas de site");
  });

  it("shows nothing when no permit is hidden", () => {
    render(<PermitHiddenNotice outsideScope={0} withoutSite={0} />);
    expect(screen.queryByRole("status")).toBeNull();
  });
});

describe("permit without a site", () => {
  it("is read at node 0 and flagged", () => {
    expect(permitWithoutSite({ entity_id: 0 })).toBe(true);
    expect(permitAccessNode({ entity_id: 0 })).toBe(0);
    expect(permitAccessNode({ entity_id: 7 })).toBe(7);
    expect(permitAccessNode(null)).toBeNull();
    render(<PermitNoSiteBadge permit={{ entity_id: 0 }} />);
    expect(screen.getByText("To review: no site")).toBeTruthy();
  });

  it("shows no badge for a permit with a site", () => {
    render(<PermitNoSiteBadge permit={{ entity_id: 5 }} />);
    expect(screen.queryByText("To review: no site")).toBeNull();
  });
});

describe("permit buttons follow the rights at the permit's site", () => {
  function renderPanel(p: WorkPermitRecord) {
    render(
      <MemoryRouter>
        <PermitDetailPanel permit={p} onChanged={vi.fn()} />
      </MemoryRouter>,
    );
  }

  it("a site-A user can suspend and cancel a site-A permit", async () => {
    rightsAt.byNode.set(3, [P.PTW_VIEW, P.PTW_ISSUE, P.PTW_CLOSE]);
    renderPanel(permit({ entity_id: 3 }));
    expect(await screen.findByRole("button", { name: "Suspend" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel permit" })).toBeTruthy();
    expect(rightsAt.asked).toContain(3);
  });

  it("the same user has neither button on a site-B permit", async () => {
    rightsAt.byNode.set(3, [P.PTW_VIEW, P.PTW_ISSUE, P.PTW_CLOSE]);
    renderPanel(permit({ entity_id: 4 }));
    await screen.findByText("PTW-2026-000041", { exact: false });
    expect(screen.queryByRole("button", { name: "Suspend" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Cancel permit" })).toBeNull();
    expect(rightsAt.asked).toContain(4);
  });

  it("hides the action buttons while the rights load", async () => {
    rightsAt.byNode.set(3, [P.PTW_VIEW, P.PTW_ISSUE, P.PTW_CLOSE]);
    rightsAt.loading = true;
    const p = permit({ entity_id: 3 });
    const onChanged = vi.fn();
    // A new element each time, so the rerender renders again.
    const ui = () => (
      <MemoryRouter>
        <PermitDetailPanel permit={p} onChanged={onChanged} />
      </MemoryRouter>
    );
    const { rerender } = render(ui());
    await screen.findByText("PTW-2026-000041", { exact: false });
    expect(screen.queryByRole("button", { name: "Suspend" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Cancel permit" })).toBeNull();
    // The same rights once loaded: the buttons come.
    rightsAt.loading = false;
    rerender(ui());
    expect(await screen.findByRole("button", { name: "Suspend" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel permit" })).toBeTruthy();
  });

  it("a permit without a site follows the plant-wide rights only, and is flagged", async () => {
    rightsAt.byNode.set(3, [P.PTW_VIEW, P.PTW_ISSUE, P.PTW_CLOSE]);
    rightsAt.byNode.set(0, [P.PTW_VIEW, P.PTW_ISSUE]);
    renderPanel(permit({ entity_id: 0 }));
    expect(await screen.findByRole("button", { name: "Suspend" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Cancel permit" })).toBeNull();
    expect(screen.getByText("To review: no site")).toBeTruthy();
    expect(rightsAt.asked).toContain(0);
  });
});

describe("hand-over receivers", () => {
  it("says why nobody can receive when the site has no holder of the close right", async () => {
    svc.listPermitHandoverReceivers.mockResolvedValue({
      items: [],
      total: 0,
      empty_reason: "PERMIT_HANDOVER_NO_RECEIVER_AT_SITE",
    });
    render(<PermitHandoverForm permitId={41} onRecorded={vi.fn()} />);
    expect(await screen.findByTestId("handover-receivers-count")).toHaveTextContent(
      enErrors.scope.PERMIT_HANDOVER_NO_RECEIVER_AT_SITE,
    );
    expect(screen.queryByText(enPermits.handover.receiversNone)).toBeNull();
  });

  it("keeps the plain nobody-matches text when there is no reason", async () => {
    svc.listPermitHandoverReceivers.mockResolvedValue({
      items: [],
      total: 0,
      empty_reason: null,
    });
    render(<PermitHandoverForm permitId={41} onRecorded={vi.fn()} />);
    expect(await screen.findByTestId("handover-receivers-count")).toHaveTextContent(
      enPermits.handover.receiversNone,
    );
  });
});

describe("create dialog", () => {
  const types = [
    { id: 1, name: "LOTO", max_duration_hours: 8, description: null },
  ] as unknown as Parameters<typeof PermitCreateDialog>[0]["types"];

  function renderDialog() {
    render(
      <PermitCreateDialog
        open
        onOpenChange={vi.fn()}
        types={types}
        workOrder={null}
        onCreated={vi.fn()}
      />,
    );
  }

  it("refuses the equipment's site when the issue right is not held there", async () => {
    rightsAt.byNode.set(3, [P.PTW_ISSUE]);
    renderDialog();
    fireEvent.click(screen.getByText("pick-site-4"));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(enPermits.siteScope.noIssueRightAtSite),
    );
    expect(screen.getByRole("button", { name: enPermits.create.submit })).toBeDisabled();
  });

  it("disables submit while the rights at the chosen site load", () => {
    rightsAt.byNode.set(3, [P.PTW_ISSUE]);
    renderDialog();
    rightsAt.loading = true;
    fireEvent.click(screen.getByText("pick-site-3"));
    expect(screen.getByRole("button", { name: enPermits.create.submit })).toBeDisabled();
  });

  it("allows the equipment's site when the issue right is held there", () => {
    rightsAt.byNode.set(3, [P.PTW_ISSUE]);
    renderDialog();
    fireEvent.click(screen.getByText("pick-site-3"));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: enPermits.create.submit })).toBeEnabled();
  });
  describe("work-order permit gate card", () => {
    function renderCard(siteId?: number | null) {
      svc.getPermitForWorkOrder.mockResolvedValue(null);
      render(
        <MemoryRouter>
          <WoPermitGateCard workOrderId={9} workOrderSiteId={siteId} />
        </MemoryRouter>,
      );
    }

    it("renders nothing while the rights load", async () => {
      rightsAt.byNode.set(3, [P.PTW_VIEW, P.PTW_ISSUE]);
      rightsAt.loading = true;
      svc.getPermitForWorkOrder.mockResolvedValue(null);
      // A new element each time, so the rerender renders again.
      const ui = () => (
        <MemoryRouter>
          <WoPermitGateCard workOrderId={9} workOrderSiteId={3} />
        </MemoryRouter>
      );
      const { rerender } = render(ui());
      await waitFor(() => expect(svc.getPermitForWorkOrder).toHaveBeenCalled());
      // Let the permit load settle: only the rights are still loading.
      await act(async () => {
        await Promise.resolve();
      });
      expect(screen.queryByTestId("wo-permit-gate")).toBeNull();
      // The same rights once loaded: the card comes.
      rightsAt.loading = false;
      rerender(ui());
      expect(await screen.findByTestId("wo-permit-gate")).toBeTruthy();
    });

    it("asks the work order's site and offers create when the issue right is held there", async () => {
      rightsAt.byNode.set(3, [P.PTW_VIEW, P.PTW_ISSUE]);
      renderCard(3);
      expect(await screen.findByRole("button", { name: enPermits.woGate.create })).toBeTruthy();
      expect(rightsAt.asked).toContain(3);
    });

    it("offers no create button when the issue right is held on another site only", async () => {
      rightsAt.byNode.set(3, [P.PTW_VIEW]);
      rightsAt.byNode.set(4, [P.PTW_ISSUE]);
      renderCard(3);
      await screen.findByTestId("wo-permit-gate");
      expect(screen.queryByRole("button", { name: enPermits.woGate.create })).toBeNull();
    });

    it.each([[undefined], [null], [0]])("asks node 0 when the site is %s", async (site) => {
      rightsAt.byNode.set(0, [P.PTW_VIEW, P.PTW_ISSUE]);
      renderCard(site);
      expect(await screen.findByRole("button", { name: enPermits.woGate.create })).toBeTruthy();
      expect(rightsAt.asked.every((n) => n === 0)).toBe(true);
    });
  });
});
