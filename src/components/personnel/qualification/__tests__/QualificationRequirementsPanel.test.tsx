import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Children, type ReactElement, type ReactNode, isValidElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enTraining from "@/i18n/locale-data/en/training.json";
import frTraining from "@/i18n/locale-data/fr/training.json";
import type { CertificationType, PersonnelSkillReferenceValue } from "@shared/ipc-types";

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

const stepUp = {
  withStepUp: (action: () => Promise<unknown>) => action(),
  StepUpDialogElement: null,
};
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => stepUp,
}));

// Radix Select is replaced by a native one (keyed on the trigger's id) so the test picks values.
vi.mock("@/components/ui/select", () => ({
  Select: ({
    value,
    onValueChange,
    children,
  }: {
    value: string;
    onValueChange: (v: string) => void;
    children: ReactNode;
  }) => {
    const trigger = Children.toArray(children).find(
      (c): c is ReactElement<{ id?: string }> => isValidElement(c) && "id" in (c.props as object),
    );
    return (
      <select id={trigger?.props.id} value={value} onChange={(e) => onValueChange(e.target.value)}>
        <option value="" />
        {children}
      </select>
    );
  },
  SelectTrigger: () => null,
  SelectValue: () => null,
  SelectContent: ({ children }: { children: ReactNode }) => <>{children}</>,
  SelectItem: ({ value, children }: { value: string; children: ReactNode }) => (
    <option value={value}>{children}</option>
  ),
}));

const listRequirements = vi.fn();
const upsertRequirement = vi.fn();
vi.mock("@/services/qualification-gate-service", () => ({
  LOTO_ROLES: ["isolation_apply", "isolation_verify", "removal_verify"],
  listQualificationRequirements: (...args: unknown[]) => listRequirements(...args),
  upsertQualificationRequirement: (...args: unknown[]) => upsertRequirement(...args),
}));
vi.mock("@/services/wo-service", () => ({
  listWorkOrderTypes: vi.fn().mockResolvedValue([
    { code: "PM", label: "Preventive", is_active: true },
    { code: "OLD", label: "Retired type", is_active: false },
  ]),
}));
vi.mock("@/services/permit-service", () => ({
  listPermitTypes: vi.fn().mockResolvedValue([]),
}));

import { QualificationRequirementsPanel } from "../QualificationRequirementsPanel";

const certTypes = [
  { id: 7, name: "Electrical habilitation", is_active: true },
  { id: 8, name: "Retired cert", is_active: false },
] as unknown as CertificationType[];
const skills = [{ id: 21, label: "Welding" }] as unknown as PersonnelSkillReferenceValue[];

function field(id: string): HTMLElement {
  return document.getElementById(id) as HTMLElement;
}

async function openForm() {
  render(<QualificationRequirementsPanel certTypes={certTypes} skills={skills} />);
  const add = await screen.findAllByRole("button", { name: enTraining.requirements.add });
  await waitFor(() => expect(listRequirements).toHaveBeenCalled());
  fireEvent.click(add[0] as HTMLElement);
  return screen.findByRole("dialog");
}

describe("QualificationRequirementsPanel form", () => {
  beforeEach(() => {
    listRequirements.mockReset().mockResolvedValue([]);
    upsertRequirement.mockReset();
  });

  it("has FR and EN text for the form section", () => {
    expect(enTraining.requirements.formSection).toBeTruthy();
    expect(frTraining.requirements.formSection).toBeTruthy();
  });

  it("opens in the standard shell and refuses to save until a target and a certification are chosen", async () => {
    await openForm();
    expect(screen.getByText(enTraining.requirements.addTitle)).toBeInTheDocument();
    expect(screen.getByText(enTraining.requirements.formSection)).toBeInTheDocument();
    expect(screen.getByText(enTraining.requirements.formDescription)).toBeInTheDocument();
    const save = screen.getByRole("button", { name: enTraining.action.save });
    expect(save).toBeDisabled();

    fireEvent.change(field("req-target"), { target: { value: "PM" } });
    expect(save).toBeDisabled();
    expect(upsertRequirement).not.toHaveBeenCalled();
  });

  it("offers only active certifications and saves the chosen requirement", async () => {
    upsertRequirement.mockResolvedValue(undefined);
    await openForm();
    expect(screen.queryByText("Retired cert")).toBeNull();
    expect(screen.queryByText("Retired type")).toBeNull();

    fireEvent.change(field("req-target"), { target: { value: "PM" } });
    fireEvent.change(field("req-cert"), { target: { value: "7" } });
    fireEvent.change(field("req-notes"), { target: { value: "  on site  " } });
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.save }));

    await waitFor(() => expect(upsertRequirement).toHaveBeenCalledTimes(1));
    expect(upsertRequirement).toHaveBeenCalledWith({
      target_kind: "work_order_type",
      target_code: "PM",
      certification_type_id: 7,
      skill_reference_value_id: null,
      min_proficiency_level: null,
      coverage: "each",
      notes: "on site",
    });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    // the list is reloaded after a save
    await waitFor(() => expect(listRequirements.mock.calls.length).toBeGreaterThanOrEqual(2));
  });

  it("saves a skill requirement with its minimum level", async () => {
    upsertRequirement.mockResolvedValue(undefined);
    await openForm();
    fireEvent.change(field("req-target"), { target: { value: "PM" } });
    fireEvent.change(field("req-what"), { target: { value: "skill" } });
    fireEvent.change(field("req-skill"), { target: { value: "21" } });
    fireEvent.change(field("req-coverage"), { target: { value: "any" } });
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.save }));

    await waitFor(() => expect(upsertRequirement).toHaveBeenCalledTimes(1));
    expect(upsertRequirement.mock.calls[0]?.[0]).toMatchObject({
      target_code: "PM",
      certification_type_id: null,
      skill_reference_value_id: 21,
      min_proficiency_level: 3,
      coverage: "any",
    });
  });

  it("keeps the form open when the save fails", async () => {
    upsertRequirement.mockRejectedValue(new Error("boom"));
    await openForm();
    fireEvent.change(field("req-target"), { target: { value: "PM" } });
    fireEvent.change(field("req-cert"), { target: { value: "7" } });
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.save }));
    await waitFor(() => expect(upsertRequirement).toHaveBeenCalled());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("cancel closes without saving", async () => {
    await openForm();
    fireEvent.click(screen.getByRole("button", { name: enTraining.action.cancel }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(upsertRequirement).not.toHaveBeenCalled();
  });
});
