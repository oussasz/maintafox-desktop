/**
 * Absence dialog: what the user types survives parent re-renders (a new `personOptions` array
 * on every render used to reset the form while typing); absence types come from the plant's list,
 * nothing is chosen for the planner, and a type tagged "critical" makes the absence critical.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

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

import { usePersonnelListsStore } from "@/stores/personnel-lists-store";
import type { PersonnelOption } from "@shared/ipc-types";

function opt(code: string, label: string, extra: Partial<PersonnelOption> = {}): PersonnelOption {
  return { code, label, label_en: null, tag: null, review_required: false, ...extra };
}

function withAbsenceTypes(absence_types: PersonnelOption[]) {
  usePersonnelListsStore.setState({ options: null, loading: false, error: null });
  svc.listPersonnelReferenceOptions.mockResolvedValue({
    employment_statuses: [],
    employment_types: [],
    absence_types,
    position_categories: [],
  });
}

import { AvailabilityBlockDialog } from "../AvailabilityBlockDialog";

function dialog(people: { id: number; label: string }[]) {
  return (
    <AvailabilityBlockDialog
      open
      onOpenChange={vi.fn()}
      mode="create"
      personOptions={people}
      onSaved={vi.fn()}
    />
  );
}

async function chooseType(label: string) {
  const type = screen.getByRole("combobox", { name: "availability.form.blockType" });
  await waitFor(() => expect(type).toBeEnabled());
  fireEvent.keyDown(type, { key: "Enter" });
  fireEvent.click(await screen.findByRole("option", { name: label }));
}

describe("AvailabilityBlockDialog", () => {
  it("proposes no person and no absence type: saving says what is missing", async () => {
    withAbsenceTypes([opt("leave", "Congé", { tag: "leave" })]);
    render(dialog([{ id: 1, label: "Amine" }]));
    await waitFor(() => expect(svc.listPersonnelReferenceOptions).toHaveBeenCalled());
    const type = screen.getByRole("combobox", { name: "availability.form.blockType" });
    await waitFor(() => expect(type).toHaveTextContent("ds.state.choose"));
    expect(screen.getByRole("combobox", { name: /availability\.form\.personnel/ })).toHaveValue("");

    fireEvent.click(screen.getByRole("button", { name: "availability.form.submit" }));
    expect(
      await screen.findByText("availability.form.errors.personnelRequired"),
    ).toBeInTheDocument();

    fireEvent.change(screen.getByRole("combobox", { name: /availability\.form\.personnel/ }), {
      target: { value: "1" },
    });
    fireEvent.click(screen.getByRole("button", { name: "availability.form.submit" }));
    expect(await screen.findByText("availability.form.errors.typeRequired")).toBeInTheDocument();
    expect(svc.createAvailabilityBlock).not.toHaveBeenCalled();
  });

  it("a chosen type tagged critical makes the absence critical", async () => {
    withAbsenceTypes([
      opt("Conge", "Congé", { tag: "leave", review_required: true }),
      opt("medical", "Médical", { tag: "critical" }),
    ]);
    render(dialog([{ id: 1, label: "Amine" }]));
    const critical = screen.getByRole("checkbox");
    expect(critical).not.toBeChecked();
    await chooseType("Médical");
    // "medical" is tagged critical: the absence is critical and the box cannot be unticked.
    await waitFor(() => expect(critical).toBeChecked());
    expect(critical).toBeDisabled();
  });

  it("leaves the critical box free for a type that is not tagged critical", async () => {
    withAbsenceTypes([opt("leave", "Congé", { tag: "leave" })]);
    render(dialog([{ id: 1, label: "Amine" }]));
    await chooseType("Congé");
    expect(screen.getByRole("combobox", { name: "availability.form.blockType" })).toHaveTextContent(
      "Congé",
    );
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByRole("checkbox")).toBeEnabled();
  });

  it("keeps the typed reason when the parent re-renders with a new people list", () => {
    withAbsenceTypes([opt("leave", "Congé", { tag: "leave" })]);
    const { rerender } = render(dialog([{ id: 1, label: "Amine" }]));
    const reason = screen.getByLabelText("availability.form.reason");
    fireEvent.change(reason, { target: { value: "Congé annuel" } });

    // Same people, new array identity (what the calendar does on every render).
    rerender(dialog([{ id: 1, label: "Amine" }]));

    expect(screen.getByLabelText("availability.form.reason")).toHaveValue("Congé annuel");
  });

  it("an edit sends back the version it read, so a block changed since is refused", async () => {
    withAbsenceTypes([opt("leave", "Congé", { tag: "leave" })]);
    svc.updateAvailabilityBlock.mockResolvedValue(undefined);
    render(
      <AvailabilityBlockDialog
        open
        onOpenChange={vi.fn()}
        mode="edit"
        personOptions={[{ id: 1, label: "Amine" }]}
        editingBlock={{
          id: 9,
          personnel_id: 1,
          block_type: "leave",
          start_at: "2026-10-19T08:00:00Z",
          end_at: "2026-10-19T16:00:00Z",
          reason_note: null,
          is_critical: false,
          created_by_id: null,
          created_by_label: null,
          created_at: "2026-10-01T00:00:00Z",
          row_version: 4,
          reason_masked: false,
          type_masked: false,
          critical_for_planning: false,
        }}
        onSaved={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "availability.form.save" }));
    await waitFor(() =>
      expect(svc.updateAvailabilityBlock).toHaveBeenCalledWith(
        expect.objectContaining({ id: 9, expected_row_version: 4 }),
      ),
    );
  });
});
