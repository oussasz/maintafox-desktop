import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { StickyActionBar } from "@/components/ui/sticky-action-bar";

describe("StickyActionBar", () => {
  it("pins the actions to the bottom of the scrolling area", () => {
    render(
      <StickyActionBar>
        <button type="button">Enregistrer</button>
      </StickyActionBar>,
    );
    const bar = screen.getByText("Enregistrer").parentElement?.parentElement;
    expect(bar?.className).toContain("sticky");
    expect(bar?.className).toContain("bottom-0");
  });

  it("says why the main action is unavailable", () => {
    render(
      <StickyActionBar hint="À compléter : titre">
        <button type="button" disabled>
          Créer
        </button>
      </StickyActionBar>,
    );
    expect(screen.getByRole("status").textContent).toBe("À compléter : titre");
  });
});
