/**
 * L6-03: the "← Retour" button on detail screens opened from another screen.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BackToOriginButton, hasScreenToReturnTo } from "@/components/ui/back-to-origin-button";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => (key === "action.back" ? "Retour" : key) }),
}));

describe("BackToOriginButton", () => {
  it("shows Retour and calls back when clicked", () => {
    const onClick = vi.fn();
    render(<BackToOriginButton onClick={onClick} />);
    fireEvent.click(screen.getByRole("button", { name: "Retour" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});

describe("hasScreenToReturnTo", () => {
  afterEach(() => {
    window.history.replaceState(null, "");
  });

  it("is false on the first screen of the session", () => {
    window.history.replaceState({ idx: 0 }, "");
    expect(hasScreenToReturnTo()).toBe(false);
  });

  it("is false when the history carries no index", () => {
    window.history.replaceState(null, "");
    expect(hasScreenToReturnTo()).toBe(false);
  });

  it("is true once the user has moved through at least one screen", () => {
    window.history.replaceState({ idx: 2 }, "");
    expect(hasScreenToReturnTo()).toBe(true);
  });
});
