import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AuthShell } from "@/components/auth/shell/AuthShell";
import { mfAuth } from "@/design-system/tokens";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));

vi.mock("@/components/auth/shell/AuthPrimitives", () => ({
  AuthLocaleSwitch: () => null,
}));

/**
 * The window root is `overflow: hidden`, so every pre-login screen must be its own scroll
 * container, and centre with flexible spacers (not `justify-center`, which clips the top of
 * content taller than the window).
 */
function expectScrollableCentred(className: string) {
  const classes = className.split(/\s+/);
  expect(classes).toContain("overflow-y-auto");
  expect(classes).toContain("h-[100dvh]");
  expect(classes).toContain("before:flex-1");
  expect(classes).toContain("after:flex-1");
  expect(classes).not.toContain("justify-center");
}

describe("pre-login full-screen shells", () => {
  it.each(["shell", "lockShell", "startupShell"] as const)("%s scrolls and centres", (key) => {
    expectScrollableCentred(mfAuth[key]);
  });

  it("AuthShell (login, activation, admin setup, password change) renders the scroll container", () => {
    const { container } = render(
      <AuthShell>
        <p>contenu</p>
      </AuthShell>,
    );
    const root = container.firstElementChild as HTMLElement;
    expectScrollableCentred(root.className);
    expect(root.querySelector("main")).not.toBeNull();
  });
});
