import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { WoDepannageForm } from "@/components/wo/WoDepannageDialog";
import { P } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts && "code" in opts ? `${key}:${String(opts["code"])}` : key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 1 } }) }));
const perms = vi.hoisted(() => ({ emergency: false }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => (p === P.OT_EMERGENCY_ASSIGN ? perms.emergency : true),
  }),
}));
const svc = vi.hoisted(() => ({ listWos: vi.fn(), startDepannageWo: vi.fn() }));
vi.mock("@/services/wo-service", () => svc);
vi.mock("@/components/wo/use-wo-people", () => ({
  useWoPeopleOptions: () => ({ options: [], loading: false, failed: false }),
}));
vi.mock("@/stores/work-order-types-catalog-store", () => ({
  useWorkOrderTypesCatalog: (sel: (s: unknown) => unknown) =>
    sel({ types: [], load: () => Promise.resolve() }),
}));
vi.mock("@/components/common/urgency/UrgencySelect", () => ({ UrgencySelect: () => null }));
vi.mock("@/components/wo/WoPermitRequirementField", () => ({
  WoPermitRequirementField: () => null,
}));
vi.mock("@/components/assets/AssetPicker", () => ({
  AssetPicker: ({ onChange }: { onChange: (a: { id: number; asset_code: string }) => void }) => (
    <button type="button" onClick={() => onChange({ id: 9, asset_code: "P-01" })}>
      pick-asset
    </button>
  ),
}));

const wo = (id: number, code: string) => ({ id, code, title: `t${id}`, status_code: "ready" });

beforeEach(() => {
  vi.clearAllMocks();
  svc.listWos.mockResolvedValue({ items: [], total: 0 });
});

function fillRequired() {
  fireEvent.click(screen.getByText("pick-asset"));
  fireEvent.click(screen.getByText("depannage.machineStopped.yes"));
  fireEvent.change(screen.getByPlaceholderText("depannage.titlePlaceholder"), {
    target: { value: "  Fuite huile  " },
  });
}

describe("WoDepannageForm", () => {
  it("keeps the launch greyed until machine stopped is answered (no default)", () => {
    render(<WoDepannageForm onClose={vi.fn()} onOpenWo={vi.fn()} />);
    fireEvent.click(screen.getByText("pick-asset"));
    fireEvent.change(screen.getByPlaceholderText("depannage.titlePlaceholder"), {
      target: { value: "Fuite" },
    });
    expect(screen.getByTestId("depannage-submit")).toBeDisabled();
    fireEvent.click(screen.getByText("depannage.machineStopped.no"));
    expect(screen.getByTestId("depannage-submit")).toBeEnabled();
  });

  it("sends the facts with the breakdown time defaulted to now, and shows the started outcome", async () => {
    svc.startDepannageWo.mockResolvedValue({
      wo: wo(5, "OT-0005"),
      breakdown_old_warning: false,
      outcome: "started",
      required_permit_types: [],
      permit_draft_ids: [],
    });
    const onCreated = vi.fn();
    const before = Date.now();
    render(<WoDepannageForm onClose={vi.fn()} onOpenWo={vi.fn()} onCreated={onCreated} />);
    fillRequired();
    fireEvent.click(screen.getByTestId("depannage-submit"));
    await waitFor(() => expect(svc.startDepannageWo).toHaveBeenCalledTimes(1));
    const input = svc.startDepannageWo.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(input).toMatchObject({
      equipment_id: 9,
      title: "Fuite huile",
      machine_stopped: true,
      type_code: null,
      assignee_id: null,
      requires_permit: false,
    });
    // Minute precision: no earlier than the minute the form opened, never after now.
    const at = new Date(String(input["breakdown_at"])).getTime();
    expect(at).toBeGreaterThan(before - 120_000);
    expect(at).toBeLessThanOrEqual(Date.now());
    expect(await screen.findByText("depannage.result.started:OT-0005")).toBeInTheDocument();
    expect(onCreated).toHaveBeenCalled();
  });

  it("explains a dépannage left waiting for its permit", async () => {
    svc.startDepannageWo.mockResolvedValue({
      wo: wo(6, "OT-0006"),
      breakdown_old_warning: true,
      outcome: "waiting_for_permit",
      required_permit_types: ["HOT_WORK"],
      permit_draft_ids: [11],
    });
    render(<WoDepannageForm onClose={vi.fn()} onOpenWo={vi.fn()} />);
    fillRequired();
    fireEvent.click(screen.getByTestId("depannage-submit"));
    expect(await screen.findByText("depannage.result.waiting:OT-0006")).toBeInTheDocument();
    expect(screen.getByText("depannage.result.permitTypes")).toBeInTheDocument();
    expect(screen.getByText("depannage.result.draftsWritten")).toBeInTheDocument();
    expect(screen.getByText("depannage.result.issuersNotified")).toBeInTheDocument();
    expect(screen.getByText("depannage.result.oldBreakdown")).toBeInTheDocument();
  });

  it("lists the open work orders on the equipment and opens one instead", async () => {
    svc.listWos.mockResolvedValue({ items: [wo(3, "OT-0003")], total: 1 });
    const onOpenWo = vi.fn();
    render(<WoDepannageForm onClose={vi.fn()} onOpenWo={onOpenWo} />);
    fireEvent.click(screen.getByText("pick-asset"));
    await screen.findByTestId("depannage-open-wos");
    expect(svc.listWos).toHaveBeenCalledWith(
      expect.objectContaining({ equipment_id: 9, not_closed_only: true }),
    );
    fireEvent.click(screen.getByText("depannage.openWos.open"));
    expect(onOpenWo).toHaveBeenCalledWith(3);
  });

  it("offers only the user as assignee without the emergency assignment right", () => {
    perms.emergency = false;
    render(<WoDepannageForm onClose={vi.fn()} onOpenWo={vi.fn()} />);
    expect(screen.getByText("depannage.assignee.onlyMe")).toBeInTheDocument();
  });
});
