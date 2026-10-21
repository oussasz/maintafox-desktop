/**
 * Settings confirmations go through the app-wide toast (the shell's host), not a toast list of
 * the page's own.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ThemeProvider } from "@/components/ui/ThemeProvider";
import { SettingsPage } from "@/pages/SettingsPage";
import { useAppToastStore } from "@/store/app-toast-store";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en", resolvedLanguage: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
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

describe("SettingsPage confirmations", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useAppToastStore.setState({ items: [] });
    svc.getSettingsOverview.mockResolvedValue({ definitions: [], roles_allowed_to_change: [] });
    svc.ensureSettingsDefaults.mockResolvedValue(undefined);
    svc.listReferenceCurrencies.mockResolvedValue([]);
    svc.listSettingChangeEvents.mockResolvedValue([]);
  });

  it("confirms restoring the defaults through the app-wide toast", async () => {
    render(
      <ThemeProvider>
        <MemoryRouter>
          <SettingsPage />
        </MemoryRouter>
      </ThemeProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "page.restoreDefaults" }));
    await waitFor(() =>
      expect(useAppToastStore.getState().items).toEqual([
        expect.objectContaining({ title: "page.restoreDefaultsSuccess", variant: "success" }),
      ]),
    );
    // No toast list of the page's own.
    expect(screen.queryByText("page.restoreDefaultsSuccess")).not.toBeInTheDocument();
  });
});
