import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

function Harness({ confirmDiscard = true }: { confirmDiscard?: boolean }) {
  const [open, setOpen] = useState(true);
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent confirmDiscard={confirmDiscard}>
        <DialogTitle>Fiche</DialogTitle>
        <input aria-label="motif" />
        <input aria-label="chercher" type="search" />
      </DialogContent>
    </Dialog>
  );
}

describe("Dialog discard guard", () => {
  it("closes on Escape when nothing was typed", () => {
    render(<Harness />);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("asks before Escape drops typed text, and keeps the text when the user continues", () => {
    render(<Harness />);
    const field = screen.getByLabelText("motif") as HTMLInputElement;
    fireEvent.change(field, { target: { value: "fuite" } });
    fireEvent.input(field, { target: { value: "fuite" } });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.getByRole("alertdialog")).toBeTruthy();
    fireEvent.click(screen.getByText("discardGuard.keep"));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect((screen.getByLabelText("motif") as HTMLInputElement).value).toBe("fuite");
  });

  it("leaves only after the user confirms", () => {
    render(<Harness />);
    fireEvent.input(screen.getByLabelText("motif"), { target: { value: "x" } });
    fireEvent.click(screen.getByLabelText("action.close"));
    fireEvent.click(screen.getByText("discardGuard.leave"));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("ignores a search box and can be turned off", () => {
    const { unmount } = render(<Harness />);
    fireEvent.input(screen.getByLabelText("chercher"), { target: { value: "pompe" } });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    unmount();
    render(<Harness confirmDiscard={false} />);
    fireEvent.input(screen.getByLabelText("motif"), { target: { value: "x" } });
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
