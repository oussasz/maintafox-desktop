import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enDashboard from "@/i18n/locale-data/en/dashboard.json";
import frDashboard from "@/i18n/locale-data/fr/dashboard.json";
import { DEFAULT_DASHBOARD_LAYOUT } from "@shared/dashboard-layout";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enDashboard,
    );
  return typeof value === "string" ? value : undefined;
}

const t = (key: string, opts?: Record<string, unknown>) =>
  (
    lookup(key) ?? (typeof opts?.["defaultValue"] === "string" ? opts["defaultValue"] : key)
  ).replace(/\{\{(\w+)\}\}/g, (_, n: string) => String(opts?.[n] ?? ""));
const i18n = { language: "en" };
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t, i18n }),
}));

// Stable functions: the dialog resets its draft when the permission helpers change identity.
const permissions = { can: () => true, canAny: () => true };
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => permissions,
}));

const saveLayoutMock = vi.fn();
const saveKpiMock = vi.fn();
vi.mock("@/services/dashboard-service", () => ({
  saveDashboardLayout: (...args: unknown[]) => saveLayoutMock(...args),
  saveOverviewKpiLayout: (...args: unknown[]) => saveKpiMock(...args),
}));

import { DashboardCustomizeDialog } from "../DashboardCustomizeDialog";

function renderDialog() {
  const onOpenChange = vi.fn();
  const onSaved = vi.fn();
  render(
    <DashboardCustomizeDialog
      open
      onOpenChange={onOpenChange}
      layout={DEFAULT_DASHBOARD_LAYOUT}
      overviewKpiLayout={[]}
      onSaved={onSaved}
      onOverviewKpiSaved={vi.fn()}
    />,
  );
  return { onOpenChange, onSaved };
}

describe("DashboardCustomizeDialog", () => {
  beforeEach(() => {
    saveLayoutMock.mockReset().mockResolvedValue(undefined);
    saveKpiMock.mockReset().mockResolvedValue(undefined);
  });

  it("has FR and EN text for the shell labels", () => {
    for (const bundle of [enDashboard, frDashboard]) {
      expect(bundle.layout.cancel).toBeTruthy();
      expect(bundle.layout.pageSectionsHeading).toBeTruthy();
      expect(bundle.layout.save).toBeTruthy();
      expect(bundle.layout.reset).toBeTruthy();
    }
  });

  it("shows the two sections and the footer actions in the standard shell", () => {
    renderDialog();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(enDashboard.layout.customizeTitle)).toBeInTheDocument();
    expect(screen.getByText(enDashboard.layout.pageSectionsHeading)).toBeInTheDocument();
    expect(screen.getByText(enDashboard.layout.metricCardsHeading)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: enDashboard.layout.reset })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: enDashboard.layout.cancel })).toBeInTheDocument();
  });

  it("saves the page and the overview cards, then closes", async () => {
    const { onOpenChange, onSaved } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: enDashboard.layout.save }));
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    expect(saveLayoutMock).toHaveBeenCalledTimes(1);
    expect(saveKpiMock).toHaveBeenCalledTimes(1);
    expect(onSaved).toHaveBeenCalledTimes(1);
  });

  it("closes without saving on cancel", () => {
    const { onOpenChange } = renderDialog();
    fireEvent.click(screen.getByRole("button", { name: enDashboard.layout.cancel }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(saveLayoutMock).not.toHaveBeenCalled();
  });
});
