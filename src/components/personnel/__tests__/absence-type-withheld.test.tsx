/**
 * Absence types withheld (decision register 2026-10-19, Q2 and (b)): without the sensitive-data
 * right the server sends blocks without their type (`type_masked`); the personnel screens show the
 * neutral tag "Absent" / "Absent — critical for planning", and a per.manage user editing such a
 * block changes dates and the critical flag while the hidden type and reason are kept (never sent).
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

import enPersonnel from "@/i18n/locale-data/en/personnel.json";
import frPersonnel from "@/i18n/locale-data/fr/personnel.json";
import type { PersonnelAvailabilityBlock } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/components/PermissionGate", () => ({
  PermissionGate: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

const svc = vi.hoisted(() => ({
  createAvailabilityBlock: vi.fn(),
  updateAvailabilityBlock: vi.fn(),
  isAvailabilityOverlapConfirmError: () => false,
  listPersonnelReferenceOptions: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => svc);

import { AvailabilityBlockDialog } from "../AvailabilityBlockDialog";
import { absenceBlockState, absenceBlockTitle } from "../absence-block-display";

const t = ((key: string) => key) as unknown as Parameters<typeof absenceBlockTitle>[2];
const lists = {
  text: (_list: string, code: string | null | undefined) => (code === "medical" ? "Médical" : ""),
  option: (_list: string, code: string | null | undefined) =>
    code === "medical"
      ? { code, label: "Médical", label_en: null, tag: "critical", review_required: false }
      : null,
};

function block(extra: Partial<PersonnelAvailabilityBlock>): PersonnelAvailabilityBlock {
  return {
    id: 5,
    personnel_id: 7,
    start_at: "2026-10-19T08:00:00Z",
    end_at: "2026-10-19T16:00:00Z",
    reason_note: null,
    is_critical: false,
    created_by_id: null,
    created_by_label: null,
    created_at: "2026-10-01T00:00:00Z",
    reason_masked: true,
    type_masked: true,
    critical_for_planning: false,
    ...extra,
  };
}

describe("Absence blocks without their type", () => {
  it("are named and coloured by the neutral tag only", () => {
    const critical = block({ critical_for_planning: true });
    const plain = block({});
    expect([absenceBlockTitle(critical, lists, t), absenceBlockState(critical, lists)]).toEqual([
      "availability.absenceTag.critical",
      "critical",
    ]);
    expect([absenceBlockTitle(plain, lists, t), absenceBlockState(plain, lists)]).toEqual([
      "availability.absenceTag.plain",
      "blocked",
    ]);
  });

  it("with the type, keep the plant's name and tag", () => {
    const full = block({ block_type: "medical", type_masked: false, reason_masked: false });
    expect([absenceBlockTitle(full, lists, t), absenceBlockState(full, lists)]).toEqual([
      "Médical",
      "critical",
    ]);
  });

  it("are edited without sending the hidden type or reason", async () => {
    svc.listPersonnelReferenceOptions.mockResolvedValue({
      employment_statuses: [],
      employment_types: [],
      absence_types: [],
      position_categories: [],
    });
    svc.updateAvailabilityBlock.mockResolvedValue(block({}));
    render(
      <AvailabilityBlockDialog
        open
        onOpenChange={vi.fn()}
        mode="edit"
        personOptions={[]}
        editingBlock={block({})}
        onSaved={vi.fn()}
      />,
    );
    expect(screen.getByText("availability.form.typeHidden")).toBeInTheDocument();
    expect(screen.getByText("availability.form.reasonHidden")).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "availability.form.blockType" })).toBeNull();

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "availability.form.save" }));
    await waitFor(() => expect(svc.updateAvailabilityBlock).toHaveBeenCalledTimes(1));
    const input = svc.updateAvailabilityBlock.mock.calls[0]?.[0] as Record<string, unknown>;
    expect(input).not.toHaveProperty("block_type");
    expect(input).not.toHaveProperty("reason_note");
    expect(input).toMatchObject({ id: 5, keep_reason_note: true, is_critical: true });
  });

  it("name the two tags and the hidden fields in English and French", () => {
    expect(enPersonnel.availability.absenceTag).toEqual({
      critical: "Absent — critical for planning",
      plain: "Absent",
    });
    expect(frPersonnel.availability.absenceTag).toEqual({
      critical: "Absent — critique pour la planification",
      plain: "Absent",
    });
    expect(frPersonnel.availability.form.typeHidden).toBeTruthy();
    expect(enPersonnel.availability.form.reasonHidden).toBeTruthy();
  });
});
