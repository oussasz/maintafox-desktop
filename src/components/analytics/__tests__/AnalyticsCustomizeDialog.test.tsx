import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enReports from "@/i18n/locale-data/en/reports.json";
import frReports from "@/i18n/locale-data/fr/reports.json";
import { DEFAULT_ANALYTICS_DASHBOARD_LAYOUT } from "@shared/analytics-dashboard-layout";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>(
      (node, part) => (node as Record<string, unknown> | undefined)?.[part],
      enReports,
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

const saveMock = vi.fn();
vi.mock("@/services/analytics-dashboard-service", () => ({
  saveAnalyticsDashboardLayout: (...args: unknown[]) => saveMock(...args),
}));

import { AnalyticsCustomizeDialog } from "../AnalyticsCustomizeDialog";

describe("AnalyticsCustomizeDialog", () => {
  beforeEach(() => {
    saveMock.mockReset();
    saveMock.mockResolvedValue(undefined);
  });

  it("has FR and EN text for the shell labels", () => {
    for (const bundle of [enReports, frReports]) {
      expect(bundle.hub.dashboards.widgetsSection).toBeTruthy();
      expect(bundle.hub.dashboards.cancel).toBeTruthy();
      expect(bundle.hub.dashboards.saveLayout).toBeTruthy();
    }
  });

  it("uses the standard form shell and saves the chosen layout", async () => {
    const onSaved = vi.fn();
    const onOpenChange = vi.fn();
    render(
      <AnalyticsCustomizeDialog
        open
        onOpenChange={onOpenChange}
        layout={DEFAULT_ANALYTICS_DASHBOARD_LAYOUT}
        onSaved={onSaved}
      />,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText(enReports.hub.dashboards.customizeTitle)).toBeInTheDocument();
    expect(screen.getByText(enReports.hub.dashboards.widgetsSection)).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole("checkbox")[0] as HTMLElement);
    fireEvent.click(screen.getByRole("button", { name: enReports.hub.dashboards.saveLayout }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    const saved = JSON.parse(
      (saveMock.mock.calls[0]?.[0] as { layout_json: string }).layout_json,
    ) as { widgets: { visible: boolean }[] };
    expect(saved.widgets[0]?.visible).toBe(false);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closes without saving on cancel", () => {
    const onOpenChange = vi.fn();
    render(
      <AnalyticsCustomizeDialog
        open
        onOpenChange={onOpenChange}
        layout={DEFAULT_ANALYTICS_DASHBOARD_LAYOUT}
        onSaved={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: enReports.hub.dashboards.cancel }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(saveMock).not.toHaveBeenCalled();
  });
});
