/**
 * In-app help: every signed-in user opens it; a module guide is listed and linked only when the
 * sidebar would show that module to the user (same right).
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const mockCan = vi.fn<(p: PermissionName) => boolean>();

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: mockCan }),
}));

vi.mock("@/hooks/use-module-capabilities", () => ({
  useModuleCapabilities: () => ({ capabilityMap: {}, loading: false }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr" },
  }),
}));

import { HelpPage } from "../HelpPage";

function renderPage() {
  return render(
    <MemoryRouter>
      <HelpPage />
    </MemoryRouter>,
  );
}

describe("HelpPage", () => {
  beforeEach(() => {
    mockCan.mockReset();
  });

  it("renders the intro, the good-to-know section and a linked guide for a permitted module", () => {
    mockCan.mockImplementation((p) => p === P.DI_VIEW);
    renderPage();

    expect(screen.getByText("intro.title")).toBeInTheDocument();
    expect(screen.getByText("good.title")).toBeInTheDocument();

    const card = screen.getByTestId("help-guide-requests");
    expect(card).toHaveTextContent("modules.requests.title");
    expect(card).toHaveTextContent("modules.requests.step4");
    expect(card.querySelector("a")?.getAttribute("href")).toBe("/requests");
  });

  it("neither lists nor links a module the user cannot open", () => {
    mockCan.mockImplementation((p) => p === P.DI_VIEW);
    renderPage();

    expect(screen.queryByTestId("help-guide-workOrders")).not.toBeInTheDocument();
    expect(screen.queryByTestId("help-guide-admin")).not.toBeInTheDocument();
    expect(screen.queryByTestId("help-guide-documentation")).not.toBeInTheDocument();
    expect(document.querySelector('a[href="/work-orders"]')).toBeNull();
    // Always-open guides stay.
    expect(screen.getByTestId("help-guide-dashboard")).toBeInTheDocument();
    expect(screen.getByTestId("help-guide-sync")).toBeInTheDocument();
  });

  it("follows the alternative rights of the Administration entry", () => {
    mockCan.mockImplementation((p) => p === P.ADM_ROLES);
    renderPage();
    expect(screen.getByTestId("help-guide-admin")).toBeInTheDocument();
  });

  it("filters the guides with the search box", () => {
    mockCan.mockImplementation(() => true);
    renderPage();
    expect(screen.getByTestId("help-guide-workOrders")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("page.searchLabel"), {
      target: { value: "modules.requests.title" },
    });
    expect(screen.getByTestId("help-guide-requests")).toBeInTheDocument();
    expect(screen.queryByTestId("help-guide-workOrders")).not.toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("page.searchLabel"), { target: { value: "zzz" } });
    expect(screen.getByText("page.noResults")).toBeInTheDocument();
  });
});
