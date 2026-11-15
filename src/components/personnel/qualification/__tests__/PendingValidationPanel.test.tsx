import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import frTraining from "@/i18n/locale-data/fr/training.json";
import type { PendingQualification } from "@/services/qualification-service";
import { P } from "@shared/rbac/permissions.generated";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enTraining,
    );
  return typeof value === "string" ? value : undefined;
}

const t = (key: string, opts?: Record<string, unknown>) => {
  const count = opts?.["count"];
  const plural =
    typeof count === "number" ? lookup(`${key}_${count === 1 ? "one" : "other"}`) : undefined;
  return (
    plural ??
    lookup(key) ??
    (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
  ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
};
const translation = { t, i18n: { language: "en" } };
vi.mock("react-i18next", () => ({ useTranslation: () => translation }));

const granted = new Set<string>();
const permissions = { can: (p: string) => granted.has(p) };
vi.mock("@/hooks/use-permissions", () => ({ usePermissions: () => permissions }));

const formatters = { formatDate: (d: string) => d };
vi.mock("@/hooks/use-formatters", () => ({ useFormatters: () => formatters }));

const withStepUp = vi.fn(<T,>(action: () => Promise<T>) => action());
const stepUp = { withStepUp, StepUpDialogElement: null };
vi.mock("@/hooks/use-step-up", () => ({ useStepUp: () => stepUp }));

const listMock = vi.fn();
const validateMock = vi.fn();
vi.mock("@/services/qualification-service", () => ({
  listPendingQualifications: () => listMock(),
  validatePendingQualifications: (...args: unknown[]) => validateMock(...args),
}));

import { PendingValidationPanel } from "../PendingValidationPanel";

const base = {
  personnel_id: 1,
  employee_code: "T-01",
  full_name: "Karim Elec",
  team_name: "Crew A",
  issued_at: null,
  issuing_body: null,
  certificate_ref: null,
};
const skill: PendingQualification = {
  ...base,
  kind: "skill",
  id: 10,
  item_code: "HYDRAULICS",
  item_label: "Hydraulics",
  proficiency_level: 3,
  valid_to: null,
  source_type: "import",
  row_version: null,
};
const cert: PendingQualification = {
  ...base,
  kind: "certification",
  id: 20,
  item_code: "BR",
  item_label: "Habilitation BR",
  proficiency_level: null,
  issued_at: "2026-01-10",
  valid_to: "2029-01-10",
  certificate_ref: "N-123",
  source_type: null,
  row_version: 1,
};

/** The confirm button of the open confirmation dialog. */
async function confirmButton(label: string): Promise<HTMLElement> {
  const dialog = await screen.findByRole("alertdialog").catch(() => screen.findByRole("dialog"));
  const button = Array.from(dialog.querySelectorAll("button")).find((b) => b.textContent === label);
  if (!button) throw new Error(`no "${label}" button in the dialog`);
  return button;
}

beforeEach(() => {
  granted.clear();
  listMock.mockReset();
  validateMock.mockReset();
  withStepUp.mockClear();
});

describe("PendingValidationPanel", () => {
  it("has a French and an English text for every key it uses", () => {
    expect(Object.keys(frTraining.validation).sort()).toEqual(
      Object.keys(enTraining.validation).sort(),
    );
    expect(frTraining.tabs.validation).toBeTruthy();
    expect(frTraining.errors.QUAL_VALIDATION_CHANGED).toBeTruthy();
  });

  it("shows the lines read-only without the right to certify", async () => {
    granted.add(P.TRN_VIEW);
    listMock.mockResolvedValue({ lines: [skill, cert], validator_linked: true });
    render(<PendingValidationPanel />);
    expect(await screen.findByText("Hydraulics")).toBeInTheDocument();
    expect(screen.getByText(enTraining.validation.readOnly)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /Select Hydraulics/ })).toBeNull();
  });

  it("validates the ticked certificate behind the step-up and shows a refused line's reason", async () => {
    granted.add(P.TRN_VIEW);
    granted.add(P.TRN_CERTIFY);
    listMock.mockResolvedValue({ lines: [skill, cert], validator_linked: true });
    validateMock.mockResolvedValue([
      { kind: "certification", id: 20, validated: false, code: "QUAL_VALIDATION_CHANGED" },
    ]);
    render(<PendingValidationPanel />);
    // Without per.manage the skill line cannot be ticked.
    const skillBox = await screen.findByRole("checkbox", { name: /Select Hydraulics/ });
    expect(skillBox).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: /Select Habilitation BR/ }));
    fireEvent.click(screen.getByRole("button", { name: "Validate 1 line" }));
    fireEvent.click(await confirmButton("Validate 1 line"));
    await waitFor(() => expect(validateMock).toHaveBeenCalledWith([cert]));
    expect(withStepUp).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(enTraining.errors.QUAL_VALIDATION_CHANGED)).toBeInTheDocument();
  });

  it("validates hidden lines never: only ticked lines still shown are sent", async () => {
    granted.add(P.TRN_VIEW);
    granted.add(P.TRN_CERTIFY);
    granted.add(P.PER_MANAGE);
    listMock.mockResolvedValue({ lines: [skill, cert], validator_linked: true });
    validateMock.mockResolvedValue([{ kind: "skill", id: 10, validated: true, code: null }]);
    render(<PendingValidationPanel />);
    fireEvent.click(await screen.findByRole("checkbox", { name: /Select Hydraulics/ }));
    fireEvent.click(screen.getByRole("checkbox", { name: /Select Habilitation BR/ }));
    // "Imported only" hides the certificate: it is no longer part of the validation.
    fireEvent.click(screen.getByRole("checkbox", { name: enTraining.validation.importedOnly }));
    fireEvent.click(screen.getByRole("button", { name: "Validate 1 line" }));
    fireEvent.click(await confirmButton("Validate 1 line"));
    await waitFor(() => expect(validateMock).toHaveBeenCalledWith([skill]));
    // Skills only: no step-up asked.
    expect(withStepUp).not.toHaveBeenCalled();
  });

  it("keeps skill lines unticked for an account without a personnel record", async () => {
    granted.add(P.TRN_VIEW);
    granted.add(P.TRN_CERTIFY);
    granted.add(P.PER_MANAGE);
    listMock.mockResolvedValue({ lines: [skill, cert], validator_linked: false });
    render(<PendingValidationPanel />);
    expect(await screen.findByRole("checkbox", { name: /Select Hydraulics/ })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: /Select Habilitation BR/ })).toBeEnabled();
    expect(screen.getByText(enTraining.validation.skillsNeedLink)).toBeInTheDocument();
  });

  it("drops the tick of a line whose content changed on reload", async () => {
    granted.add(P.TRN_VIEW);
    granted.add(P.TRN_CERTIFY);
    listMock.mockResolvedValue({ lines: [cert], validator_linked: true });
    const { rerender } = render(<PendingValidationPanel refreshKey={0} />);
    const box = await screen.findByRole("checkbox", { name: /Select Habilitation BR/ });
    fireEvent.click(box);
    expect(box).toBeChecked();
    // Edited elsewhere: a new version of the same certificate.
    listMock.mockResolvedValue({ lines: [{ ...cert, row_version: 2 }], validator_linked: true });
    rerender(<PendingValidationPanel refreshKey={1} />);
    await waitFor(() =>
      expect(screen.getByRole("checkbox", { name: /Select Habilitation BR/ })).not.toBeChecked(),
    );
    expect(screen.getByRole("button", { name: "Validate 0 lines" })).toBeDisabled();
  });
});
