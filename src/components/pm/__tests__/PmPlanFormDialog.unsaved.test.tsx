import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { PmPlanFormDialog } from "@/components/pm/PmPlanFormDialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
vi.mock("@/services/personnel-service", () => ({
  listActiveTeams: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@/components/assets/AssetPicker", () => ({ AssetPicker: () => null }));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));

function renderDialog() {
  const onClose = vi.fn();
  render(
    <PmPlanFormDialog
      open
      mode="create"
      onClose={onClose}
      onCreate={vi.fn(() => Promise.resolve())}
      onUpdate={vi.fn(() => Promise.resolve())}
    />,
  );
  return { onClose };
}

describe("PmPlanFormDialog unsaved input", () => {
  it("closes at once on Cancel when nothing was typed", () => {
    const { onClose } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: "actions.cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("asks on Cancel once something was typed, and closes only when confirmed", async () => {
    const { onClose } = renderDialog();
    fireEvent.change(screen.getByLabelText("fields.title"), { target: { value: "Graissage" } });
    fireEvent.click(screen.getByRole("button", { name: "actions.cancel" }));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: "discardGuard.leave" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });
});
