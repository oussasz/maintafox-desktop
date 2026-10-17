/**
 * Absence dialog: what the user types survives parent re-renders (a new `personOptions` array
 * on every render used to reset the form while typing).
 */
import { fireEvent, render, screen } from "@testing-library/react";
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

vi.mock("@/services/personnel-service", () => ({
  createAvailabilityBlock: vi.fn(),
  updateAvailabilityBlock: vi.fn(),
  isAvailabilityOverlapConfirmError: () => false,
}));

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

describe("AvailabilityBlockDialog", () => {
  it("keeps the typed reason when the parent re-renders with a new people list", () => {
    const { rerender } = render(dialog([{ id: 1, label: "Amine" }]));
    const reason = screen.getByLabelText("availability.form.reason");
    fireEvent.change(reason, { target: { value: "Congé annuel" } });

    // Same people, new array identity (what the calendar does on every render).
    rerender(dialog([{ id: 1, label: "Amine" }]));

    expect(screen.getByLabelText("availability.form.reason")).toHaveValue("Congé annuel");
  });
});
