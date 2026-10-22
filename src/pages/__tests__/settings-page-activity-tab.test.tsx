/**
 * AU-46: the activity journal is its own Settings tab on `log.view` (what `activity_feed` asks), so
 * a user who holds it without the settings right reaches it; without `log.view` there is no tab.
 */
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { SettingsPage } from "@/pages/SettingsPage";
import { P } from "@shared/rbac/permissions.generated";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en", resolvedLanguage: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

let granted: string[] = [];
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => granted.includes(p),
    canAny: (...ps: string[]) => ps.some((p) => granted.includes(p)),
    isLoading: false,
  }),
}));

vi.mock("@/components/settings/ActivityLogSettingsSection", () => ({
  ActivityLogSettingsSection: () => <div data-testid="activity-section" />,
}));

const svc = vi.hoisted(() => ({
  ensureSettingsDefaults: vi.fn(),
  getSettingsOverview: vi.fn(),
  listReferenceCurrencies: vi.fn(),
  listSettingChangeEvents: vi.fn(),
}));
vi.mock("@/services/settings-service", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...svc,
}));

function renderAt(url: string) {
  render(
    <ThemeProvider>
      <MemoryRouter initialEntries={[url]}>
        <SettingsPage />
      </MemoryRouter>
    </ThemeProvider>,
  );
}

describe("Settings activity tab (AU-46)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    svc.getSettingsOverview.mockResolvedValue({ definitions: [], roles_allowed_to_change: [] });
    svc.listReferenceCurrencies.mockResolvedValue([]);
    svc.listSettingChangeEvents.mockResolvedValue([]);
  });

  it("opens the journal with log.view alone, without the settings tabs", async () => {
    granted = [P.LOG_VIEW];
    renderAt("/settings?tab=activity");
    expect(await screen.findByTestId("activity-section")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "tabs.activity" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "tabs.security" })).not.toBeInTheDocument();
    expect(svc.getSettingsOverview).not.toHaveBeenCalled();
  });

  it("has no journal tab without log.view", async () => {
    granted = [P.ADM_SETTINGS];
    renderAt("/settings?tab=activity");
    expect(await screen.findByRole("button", { name: "tabs.security" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "tabs.activity" })).not.toBeInTheDocument();
    expect(screen.queryByTestId("activity-section")).not.toBeInTheDocument();
  });
});
