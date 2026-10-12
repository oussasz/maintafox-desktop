import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { UnitConversionsPanel, parseFactor } from "../UnitConversionsPanel";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, o?: Record<string, unknown>) => (o ? `${key}:${JSON.stringify(o)}` : key),
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({
  listUnitConversions: vi.fn(),
  createUnitConversion: vi.fn(),
  deactivateUnitConversion: vi.fn(),
  convertUnitQuantity: vi.fn(),
  updateUnitConversion: vi.fn(),
}));
vi.mock("@/services/unit-conversion-service", () => svc);
const perms = vi.hoisted(() => ({ allowed: true }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => perms.allowed, isLoading: false }),
}));

const unit = (id: number, code: string) => ({
  id,
  code,
  label: code,
  fr_label: null,
  en_label: null,
  color: null,
  is_active: 1,
});
const units = [unit(1, "U"), unit(6, "BOX")];
const conversion = {
  id: 9,
  entity_sync_id: "s",
  from_unit_value_id: 6,
  from_unit_code: "BOX",
  from_unit_label: "Box",
  to_unit_value_id: 1,
  to_unit_code: "U",
  to_unit_label: "Unit",
  article_id: null,
  article_code: null,
  factor: 12,
  notes: null,
  is_active: true,
  row_version: 3,
  updated_at: "2026-10-12T08:00:00Z",
};

beforeEach(() => {
  vi.clearAllMocks();
  perms.allowed = true;
});

function nth<T>(items: T[], i: number): T {
  const item = items[i];
  if (item === undefined) throw new Error(`missing element ${i}`);
  return item;
}

function selects() {
  const all = screen.getAllByRole("combobox");
  return { from: nth(all, 0), to: nth(all, 1), article: nth(all, 2) };
}

describe("parseFactor", () => {
  it("accepts positive numbers with a dot or a comma", () => {
    expect(parseFactor("12")).toBe(12);
    expect(parseFactor("0,5")).toBe(0.5);
    expect(parseFactor("")).toBeNull();
    expect(parseFactor("0")).toBeNull();
    expect(parseFactor("-2")).toBeNull();
    expect(parseFactor("abc")).toBeNull();
  });
});

describe("UnitConversionsPanel", () => {
  it("lists conversions, creates one and deactivates one", async () => {
    svc.listUnitConversions.mockResolvedValue([conversion]);
    svc.createUnitConversion.mockResolvedValue(conversion);
    svc.deactivateUnitConversion.mockResolvedValue({ ...conversion, is_active: false });
    render(<UnitConversionsPanel unitOptions={units} articles={[]} />);
    expect(await screen.findByText("1 BOX = 12 U")).toBeTruthy();

    const s = selects();
    fireEvent.change(s.from, { target: { value: "6" } });
    fireEvent.change(s.to, { target: { value: "1" } });
    fireEvent.change(nth(screen.getAllByRole("textbox"), 0), { target: { value: "10" } });
    fireEvent.click(screen.getByText("unitConversions.add"));
    await waitFor(() =>
      expect(svc.createUnitConversion).toHaveBeenCalledWith({
        from_unit_value_id: 6,
        to_unit_value_id: 1,
        article_id: null,
        factor: 10,
        notes: null,
      }),
    );

    fireEvent.click(screen.getByText("unitConversions.deactivate"));
    await waitFor(() => expect(svc.deactivateUnitConversion).toHaveBeenCalledWith(9, 3));
  });

  it("converts a quantity and shows conversion errors", async () => {
    svc.listUnitConversions.mockResolvedValue([]);
    svc.convertUnitQuantity
      .mockResolvedValueOnce({ quantity: 2, factor: 1 / 12, path: "inverse", conversion_id: 9 })
      .mockRejectedValueOnce(new Error("No conversion is defined between these units."));
    render(<UnitConversionsPanel unitOptions={units} articles={[]} />);
    expect(await screen.findByText("unitConversions.empty")).toBeTruthy();
    const s = selects();
    fireEvent.change(s.from, { target: { value: "1" } });
    fireEvent.change(s.to, { target: { value: "6" } });
    fireEvent.click(screen.getByText("unitConversions.convert"));
    expect((await screen.findByTestId("conversion-result")).textContent).toContain('"quantity":2');
    expect(svc.convertUnitQuantity).toHaveBeenCalledWith(1, 1, 6, null);
    fireEvent.click(screen.getByText("unitConversions.convert"));
    expect((await screen.findByRole("alert")).textContent).toContain("No conversion");
  });

  it("is read-only without the manage permission", async () => {
    perms.allowed = false;
    svc.listUnitConversions.mockResolvedValue([conversion]);
    render(<UnitConversionsPanel unitOptions={units} articles={[]} />);
    expect(await screen.findByText("1 BOX = 12 U")).toBeTruthy();
    expect(screen.queryByText("unitConversions.add")).toBeNull();
    expect(screen.queryByText("unitConversions.deactivate")).toBeNull();
  });
});
