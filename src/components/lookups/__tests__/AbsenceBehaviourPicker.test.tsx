/**
 * Absence types in Données de référence: each names a behaviour (leave / critical / training);
 * taking "critical" off a type warns that the absences already recorded stay critical.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import enReference from "@/i18n/locale-data/en/reference.json";
import frReference from "@/i18n/locale-data/fr/reference.json";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

import {
  AbsenceBehaviourSelect,
  CriticalTagRemovalWarning,
  removesCriticalTag,
} from "../AbsenceBehaviourPicker";
import { resolveReferenceValueColumns } from "../reference-value-columns";

describe("Absence-type behaviour", () => {
  it("is a column of PERSONNEL.ABSENCE_TYPES only", () => {
    const base = {
      id: 1,
      name: "x",
      structure_type: "flat",
      governance_level: "tenant_managed",
      governance_category: "operational_dictionary",
      audience: "company",
      is_extendable: true,
      validation_rules_json: null,
      created_at: "",
      updated_at: "",
    } as const;
    expect(
      resolveReferenceValueColumns({ ...base, code: "PERSONNEL.ABSENCE_TYPES" })
        .showAbsenceBehaviour,
    ).toBe(true);
    expect(
      resolveReferenceValueColumns({ ...base, code: "PERSONNEL.SKILLS" }).showAbsenceBehaviour,
    ).toBe(false);
  });

  it("proposes the three behaviours with nothing chosen", () => {
    const onChange = vi.fn();
    render(<AbsenceBehaviourSelect value={null} onChange={onChange} />);
    const select = screen.getByRole("combobox", { name: "absenceBehaviour.column" });
    expect(select).toHaveValue("");
    fireEvent.change(select, { target: { value: "critical" } });
    expect(onChange).toHaveBeenCalledWith("critical");
  });

  it("warns only when the critical tag is taken off", () => {
    expect(removesCriticalTag("critical", "leave")).toBe(true);
    expect(removesCriticalTag("critical", null)).toBe(true);
    expect(removesCriticalTag("critical", "critical")).toBe(false);
    expect(removesCriticalTag("leave", "training")).toBe(false);
    expect(removesCriticalTag(null, "critical")).toBe(false);
    render(<CriticalTagRemovalWarning />);
    expect(screen.getByRole("alert")).toHaveTextContent("absenceBehaviour.criticalRemovedWarning");
  });

  it("words the warning in French and English", () => {
    expect(frReference.absenceBehaviour.criticalRemovedWarning).toBe(
      "Les absences déjà saisies de ce type restent marquées critiques.",
    );
    expect(enReference.absenceBehaviour.criticalRemovedWarning).toBe(
      "Absences already recorded with this type stay marked critical.",
    );
  });
});
