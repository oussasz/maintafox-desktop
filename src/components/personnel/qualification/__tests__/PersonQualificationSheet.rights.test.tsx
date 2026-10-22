/**
 * AU-57: the qualification sheet without trn.view shows the certifications' missing right, while
 * skills still load and can be validated (upsert_personnel_skill: per.manage + trn.certify).
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PersonQualificationSheet } from "@/components/personnel/qualification/PersonQualificationSheet";
import { PermissionContext, type PermissionContextValue } from "@/contexts/PermissionContext";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({ withStepUp: (fn: () => unknown) => fn(), StepUpDialogElement: null }),
}));

const qual = vi.hoisted(() => ({
  listCertificationTypes: vi.fn(),
  listPersonnelCertifications: vi.fn(),
  upsertPersonnelCertification: vi.fn(),
}));
vi.mock("@/services/qualification-service", () => qual);
const workforce = vi.hoisted(() => ({
  listPersonnelSkills: vi.fn(),
  removePersonnelSkill: vi.fn(),
  upsertPersonnelSkill: vi.fn(),
}));
vi.mock("@/services/workforce-service", () => workforce);
vi.mock("@/services/personnel-service", () => ({
  listPersonnelSkillReferenceValues: vi.fn().mockResolvedValue([]),
}));

const granted: PermissionName[] = [P.PER_VIEW, P.PER_MANAGE, P.TRN_CERTIFY];
const rights: PermissionContextValue = {
  permissions: [],
  isLoading: false,
  can: (p) => granted.includes(p),
  canAny: (...ps) => ps.some((p) => granted.includes(p)),
  canAll: (...ps) => ps.every((p) => granted.includes(p)),
  refresh: async () => {},
};

describe("PersonQualificationSheet rights (AU-57)", () => {
  it("without trn.view: certifications name the right, skills load and can be validated", async () => {
    workforce.listPersonnelSkills.mockResolvedValue([
      {
        id: 1,
        reference_value_id: 4,
        skill_label: "Soudure",
        proficiency_level: 3,
        valid_to: null,
        is_primary: false,
        is_validated: false,
        verified_by_name: null,
        verified_at: null,
      },
    ]);
    render(
      <PermissionContext.Provider value={rights}>
        <PersonQualificationSheet personnelId={7} />
      </PermissionContext.Provider>,
    );
    expect(await screen.findByText("Soudure")).toBeInTheDocument();
    expect(screen.getByTestId("list-access-certificationTypes")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "sheet.validate" })).toBeInTheDocument();
    expect(qual.listCertificationTypes).not.toHaveBeenCalled();
    expect(qual.listPersonnelCertifications).not.toHaveBeenCalled();
  });
});
