import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import frTraining from "@/i18n/locale-data/fr/training.json";
import type { CertificationType } from "@shared/ipc-types";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enTraining,
    );
  return typeof value === "string" ? value : undefined;
}

// The translator and permission checker must be stable across renders, otherwise the
// dialog's draft is rebuilt on every render.
const t = (key: string, opts?: Record<string, unknown>) =>
  (
    lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
  ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
const i18n = { language: "en" };
const translation = { t, i18n };
vi.mock("react-i18next", () => ({
  useTranslation: () => translation,
}));

const permissions = { can: () => true };
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => permissions,
}));

const upsertMock = vi.fn();
vi.mock("@/services/qualification-service", () => ({
  upsertCertificationType: (...args: unknown[]) => upsertMock(...args),
}));

import { CertificationCatalogPanel } from "../CertificationCatalogPanel";

const existing = {
  id: 4,
  row_version: 3,
  code: "HAB-E",
  name: "Electrical habilitation",
  category: "electrical",
  default_validity_months: 36,
  renewal_lead_days: 60,
  description: "B1V / BR",
  is_active: true,
} as unknown as CertificationType;

function openAddForm(onChanged = vi.fn()) {
  render(<CertificationCatalogPanel certTypes={[]} loading={false} onChanged={onChanged} />);
  fireEvent.click(screen.getByRole("button", { name: enTraining.catalog.add }));
  return onChanged;
}

function field(id: string): HTMLElement {
  return document.getElementById(id) as HTMLElement;
}

describe("CertificationCatalogPanel form", () => {
  beforeEach(() => {
    upsertMock.mockReset();
  });

  it("has FR and EN text for the form section", () => {
    expect(enTraining.catalog.formSection).toBeTruthy();
    expect(frTraining.catalog.formSection).toBeTruthy();
  });

  it("opens in the standard shell with save disabled until code and name are given", async () => {
    openAddForm();
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(enTraining.catalog.addTitle)).toBeInTheDocument();
    expect(screen.getByText(enTraining.catalog.formSection)).toBeInTheDocument();
    expect(screen.getByText(enTraining.catalog.formDescription)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: enTraining.action.save })).toBeDisabled();
  });

  it("refuses a validity that is not a positive whole number", async () => {
    openAddForm();
    await screen.findByRole("dialog");
    fireEvent.change(field("ct-code"), { target: { value: "LOTO" } });
    fireEvent.change(field("ct-name"), { target: { value: "LOTO authority" } });
    expect(screen.getByRole("button", { name: enTraining.action.save })).toBeEnabled();

    fireEvent.change(field("ct-validity"), { target: { value: "-2" } });
    expect(screen.getByText(enTraining.catalog.positiveNumber)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: enTraining.action.save })).toBeDisabled();
    expect(upsertMock).not.toHaveBeenCalled();
  });

  it("saves the typed values, closes and tells the page to reload", async () => {
    upsertMock.mockResolvedValue(undefined);
    const onChanged = openAddForm();
    await screen.findByRole("dialog");
    fireEvent.change(field("ct-code"), { target: { value: " LOTO " } });
    fireEvent.change(field("ct-name"), { target: { value: "LOTO authority" } });
    fireEvent.change(field("ct-validity"), { target: { value: "24" } });
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.save }));

    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1));
    expect(upsertMock).toHaveBeenCalledWith({
      id: null,
      expected_row_version: null,
      code: "LOTO",
      name: "LOTO authority",
      category: "other",
      default_validity_months: 24,
      renewal_lead_days: null,
      description: null,
      is_active: true,
    });
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("edits an existing certification with its row version", async () => {
    upsertMock.mockResolvedValue(undefined);
    render(
      <CertificationCatalogPanel certTypes={[existing]} loading={false} onChanged={vi.fn()} />,
    );
    fireEvent.click(await screen.findByRole("button", { name: /Electrical habilitation/ }));
    expect(await screen.findByText(enTraining.catalog.editTitle)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.save }));
    await waitFor(() => expect(upsertMock).toHaveBeenCalledTimes(1));
    expect(upsertMock.mock.calls[0]?.[0]).toMatchObject({
      id: 4,
      expected_row_version: 3,
      code: "HAB-E",
      default_validity_months: 36,
      renewal_lead_days: 60,
    });
  });

  it("keeps the form open when the save fails", async () => {
    upsertMock.mockRejectedValue(new Error("boom"));
    const onChanged = openAddForm();
    await screen.findByRole("dialog");
    fireEvent.change(field("ct-code"), { target: { value: "X" } });
    fireEvent.change(field("ct-name"), { target: { value: "Y" } });
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.save }));
    await waitFor(() => expect(upsertMock).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("cancel closes without saving", async () => {
    openAddForm();
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.cancel }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(upsertMock).not.toHaveBeenCalled();
  });
});
