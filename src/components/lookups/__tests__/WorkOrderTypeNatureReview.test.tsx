import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enReference from "@/i18n/locale-data/en/reference.json";
import enTraining from "@/i18n/locale-data/en/training.json";
import { P } from "@shared/rbac/permissions.generated";

import { NATURE_REASON_MIN, WorkOrderTypeNatureReview } from "../WorkOrderTypeNatureReview";

function lookup(catalog: unknown, key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      catalog,
    );
  return typeof value === "string" ? value : undefined;
}

function fill(value: string, opts?: Record<string, unknown>) {
  return value.replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
}

vi.mock("react-i18next", () => ({
  useTranslation: (ns?: string) => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      if (ns === "common") return key;
      const plural =
        typeof opts?.["count"] === "number"
          ? `${key}_${opts["count"] === 1 ? "one" : "other"}`
          : null;
      const value = lookup(enReference, key) ?? (plural ? lookup(enReference, plural) : undefined);
      if (value == null) throw new Error(`missing reference key: ${key}`);
      return fill(value, opts);
    },
    i18n: { language: "en" },
  }),
}));

vi.mock("@/lib/app-error", () => ({
  appErrorReason: (e: { code?: string }) =>
    ((e.code && lookup(enTraining, `errors.${e.code}`)) || "unknown").replace(/\{\{\w+\}\}/g, ""),
}));

const svc = vi.hoisted(() => ({
  listWorkOrderTypesToReview: vi.fn(),
  setWorkOrderTypeNature: vi.fn(),
}));
vi.mock("@/services/wo-service", () => svc);
vi.mock("@/stores/work-order-types-catalog-store", () => ({
  refreshWorkOrderTypesCatalog: vi.fn().mockResolvedValue(undefined),
}));

// The natures come from the published reference list, not from the component.
const NATURES = {
  preventive_predetermined: "Preventive — predetermined",
  improvement: "Improvement / modification",
};
const refSvc = vi.hoisted(() => ({ listPublishedReferenceValuesByDomainCode: vi.fn() }));
vi.mock("@/services/reference-service", () => refSvc);
const natureRows = () =>
  Object.entries(NATURES).map(([code, label], i) => ({
    id: i + 1,
    set_id: 1,
    parent_id: null,
    code,
    label,
    description: null,
    sort_order: i + 1,
    color_hex: null,
    icon_name: null,
    semantic_tag: null,
    external_code: null,
    is_active: true,
    metadata_json: JSON.stringify({ label_en: label }),
  }));

const perms = vi.hoisted(() => ({ denied: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => !perms.denied.has(p), isLoading: false }),
}));

const PLANT_TYPE = {
  id: 77,
  code: "PLANT_OWN",
  label: "Type maison",
  is_active: true,
  work_order_count: 3,
};
const nr = enReference.natureReview;

async function openDialog() {
  fireEvent.click(await screen.findByRole("button", { name: nr.give }));
  return screen.findByRole("dialog");
}

describe("WorkOrderTypeNatureReview", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    perms.denied = new Set();
    svc.listWorkOrderTypesToReview.mockResolvedValue([PLANT_TYPE]);
    svc.setWorkOrderTypeNature.mockResolvedValue({});
    refSvc.listPublishedReferenceValuesByDomainCode.mockResolvedValue(natureRows());
  });

  it.each([
    ["is empty", () => refSvc.listPublishedReferenceValuesByDomainCode.mockResolvedValue([])],
    [
      "cannot be read",
      () => refSvc.listPublishedReferenceValuesByDomainCode.mockRejectedValue(new Error("x")),
    ],
  ])("says so when the list of natures %s", async (_name, arrange) => {
    arrange();
    render(<WorkOrderTypeNatureReview />);
    await openDialog();
    expect(await screen.findByText(nr.naturesUnavailable)).toBeInTheDocument();
  });

  it("shows nothing when every type has a nature", async () => {
    svc.listWorkOrderTypesToReview.mockResolvedValue([]);
    const { container } = render(<WorkOrderTypeNatureReview />);
    await waitFor(() => expect(svc.listWorkOrderTypesToReview).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("lists the types with their use; the action needs the reference manage right", async () => {
    perms.denied = new Set([P.REF_MANAGE]);
    render(<WorkOrderTypeNatureReview />);
    expect(await screen.findByText("Type maison")).toBeInTheDocument();
    expect(screen.getByText(fill(nr.workOrders_other, { count: 3 }))).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: nr.give })).not.toBeInTheDocument();
  });

  it("requires a nature (none preselected) and a reason of the minimum length", async () => {
    render(<WorkOrderTypeNatureReview />);
    await openDialog();
    fireEvent.click(screen.getByRole("button", { name: nr.save }));
    expect(await screen.findByText(nr.natureRequired)).toBeInTheDocument();
    expect(screen.getByText(fill(nr.reasonMin, { min: NATURE_REASON_MIN }))).toBeInTheDocument();
    expect(svc.setWorkOrderTypeNature).not.toHaveBeenCalled();
  });

  it("gives the nature with the reason and reloads the list", async () => {
    render(<WorkOrderTypeNatureReview />);
    await openDialog();
    fireEvent.keyDown(screen.getByRole("combobox", { name: nr.nature }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: NATURES.preventive_predetermined }));
    fireEvent.change(screen.getByLabelText(new RegExp(nr.reason)), {
      target: { value: "  Planned lubrication rounds  " },
    });
    svc.listWorkOrderTypesToReview.mockResolvedValue([]);
    fireEvent.click(screen.getByRole("button", { name: nr.save }));
    await waitFor(() =>
      expect(svc.setWorkOrderTypeNature).toHaveBeenCalledWith({
        type_id: 77,
        nature: "preventive_predetermined",
        reason: "Planned lubrication rounds",
      }),
    );
    await waitFor(() => expect(svc.listWorkOrderTypesToReview).toHaveBeenCalledTimes(2));
  });

  it.each([
    "WO_TYPE_NATURE_RIGHT_REQUIRED",
    "WO_TYPE_NATURE_REASON_REQUIRED",
    "WO_TYPE_NATURE_UNKNOWN",
    "WO_TYPE_NATURE_TYPE_NOT_FOUND",
    "WO_TYPE_NATURE_SYSTEM_TYPE",
    "WO_TYPE_NATURE_ALREADY_SET",
  ])("shows the translated refusal %s", async (code) => {
    svc.setWorkOrderTypeNature.mockRejectedValue({ code });
    render(<WorkOrderTypeNatureReview />);
    await openDialog();
    fireEvent.keyDown(screen.getByRole("combobox", { name: nr.nature }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: NATURES.improvement }));
    fireEvent.change(screen.getByLabelText(new RegExp(nr.reason)), {
      target: { value: "Plant type is an improvement" },
    });
    fireEvent.click(screen.getByRole("button", { name: nr.save }));
    const text = lookup(enTraining, `errors.${code}`);
    expect(text).toBeTruthy();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      String(text)
        .replace(/\{\{\w+\}\}/g, "")
        .replace(/\s+/g, " "),
    );
  });
});
