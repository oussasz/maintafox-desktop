import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_SCHEDULE_SPEC,
  buildScheduleCron,
  localUtcOffsetMinutes,
} from "@/components/analytics/report-schedule-cron";
import { AnalyticsHistoryPage } from "@/pages/analytics/AnalyticsHistoryPage";
import { AnalyticsSchedulesPage } from "@/pages/analytics/AnalyticsSchedulesPage";
import { AnalyticsTemplatesPage } from "@/pages/analytics/AnalyticsTemplatesPage";
import type { ReportRun, ReportSchedule, ReportTemplate } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "file" in opts ? `${key}:${String(opts["file"])}` : key,
    i18n: { language: "en" },
  }),
}));

vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => true, canAny: () => true, isLoading: false }),
}));

const svc = vi.hoisted(() => ({
  listReportTemplates: vi.fn(),
  listMyReportSchedules: vi.fn(),
  upsertMyReportSchedule: vi.fn(),
  deleteMyReportSchedule: vi.fn(),
  listMyReportRuns: vi.fn(),
  exportReportNow: vi.fn(),
  downloadReportRunArtifact: vi.fn(),
  saveExportedDocument: vi.fn(),
  toReportLocale: (l: string) => (l.startsWith("en") ? "en" : "fr"),
}));
vi.mock("@/services/reports-service", () => svc);

const TEMPLATES: ReportTemplate[] = [
  {
    id: 1,
    code: "dashboard_summary",
    title: "Dashboard summary",
    description: "KPI counts",
    default_format: "pdf",
    spec_json: "{}",
    is_active: true,
  },
];

const DOC = { file_name: "dashboard-summary.pdf", mime_type: "application/pdf", bytes: [1, 2] };

beforeEach(() => {
  vi.clearAllMocks();
  svc.listReportTemplates.mockResolvedValue(TEMPLATES);
  svc.listMyReportSchedules.mockResolvedValue([]);
  svc.upsertMyReportSchedule.mockResolvedValue(5);
  svc.deleteMyReportSchedule.mockResolvedValue(undefined);
  svc.listMyReportRuns.mockResolvedValue([]);
  svc.exportReportNow.mockResolvedValue(DOC);
  svc.downloadReportRunArtifact.mockResolvedValue(DOC);
  svc.saveExportedDocument.mockResolvedValue("C:\\Users\\me\\Downloads\\dashboard-summary.pdf");
});

describe("AnalyticsTemplatesPage", () => {
  it("shows translated template titles and saves the export with the app language", async () => {
    render(<AnalyticsTemplatesPage />);
    expect(
      await screen.findByText("templates.catalog.dashboard_summary.title"),
    ).toBeInTheDocument();
    expect(screen.queryByText("dashboard_summary")).not.toBeInTheDocument();

    fireEvent.click(
      screen.getAllByRole("button", { name: "templates.exportAria" })[0] as HTMLElement,
    );
    await waitFor(() =>
      expect(svc.exportReportNow).toHaveBeenCalledWith(
        expect.objectContaining({ template_code: "dashboard_summary", export_format: "pdf" }),
      ),
    );
    const input = svc.exportReportNow.mock.calls[0]?.[0] as { locale: string };
    expect(["fr", "en"]).toContain(input.locale);
    expect(svc.saveExportedDocument).toHaveBeenCalledWith(DOC, "save.dialogTitle");
    expect(await screen.findByText("save.saved:dashboard-summary.pdf")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "save.openFolder" })).toBeInTheDocument();
  });

  it("shows an error state with retry when templates cannot be loaded", async () => {
    svc.listReportTemplates.mockRejectedValueOnce({ code: "DATABASE_ERROR", message: "x" });
    render(<AnalyticsTemplatesPage />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "ds.state.retry" }));
    expect(
      await screen.findByText("templates.catalog.dashboard_summary.title"),
    ).toBeInTheDocument();
  });
});

describe("AnalyticsSchedulesPage", () => {
  it("adds a schedule from the plain picker as the backend cron string", async () => {
    render(<AnalyticsSchedulesPage />);
    fireEvent.click(await screen.findByRole("button", { name: "schedule.save" }));
    const expected = buildScheduleCron(DEFAULT_SCHEDULE_SPEC, localUtcOffsetMinutes());
    if (!expected.ok) throw new Error("default schedule must be valid");
    await waitFor(() =>
      expect(svc.upsertMyReportSchedule).toHaveBeenCalledWith({
        template_id: 1,
        cron_expr: expected.cron,
        export_format: "pdf",
        enabled: true,
      }),
    );
    expect(expected.cron).toMatch(/^0 \d{1,2} \d{1,2} \* \* (Sun|Mon|Tue)$/);
  });

  it("describes stored schedules in words and confirms before deleting", async () => {
    const schedule: ReportSchedule = {
      id: 7,
      user_id: 1,
      template_id: 1,
      cron_expr: "0 0 8 * * *",
      export_format: "xlsx",
      enabled: true,
      next_run_at: "2026-10-15T08:00:00+00:00",
      last_run_at: null,
    };
    svc.listMyReportSchedules.mockResolvedValue([schedule]);
    render(<AnalyticsSchedulesPage />);

    expect(await screen.findByText("schedule.describe.daily")).toBeInTheDocument();
    expect(screen.queryByText("0 0 8 * * *")).not.toBeInTheDocument();
    expect(screen.queryByText("2026-10-15T08:00:00+00:00")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "schedule.delete" }));
    expect(svc.deleteMyReportSchedule).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "schedule.deleteConfirm" }));
    await waitFor(() => expect(svc.deleteMyReportSchedule).toHaveBeenCalledWith(7));
  });

  it("shows a custom cron in technical details instead of the raw string", async () => {
    svc.listMyReportSchedules.mockResolvedValue([
      {
        id: 8,
        user_id: 1,
        template_id: 1,
        cron_expr: "0 */15 * * * *",
        export_format: "pdf",
        enabled: false,
        next_run_at: "2026-10-15T08:00:00+00:00",
        last_run_at: null,
      },
    ]);
    render(<AnalyticsSchedulesPage />);
    expect(await screen.findByText("schedule.custom")).toBeInTheDocument();
    expect(screen.getByText("schedule.paused")).toBeInTheDocument();
  });
});

describe("AnalyticsHistoryPage", () => {
  const run = (partial: Partial<ReportRun>): ReportRun => ({
    id: 11,
    schedule_id: null,
    template_id: 1,
    user_id: 1,
    status: "success",
    export_format: "pdf",
    artifact_path: "C:\\AppData\\maintafox\\reports\\11.pdf",
    byte_size: 2,
    error_message: null,
    started_at: "2026-10-14T06:30:00Z",
    finished_at: "2026-10-14T06:30:02Z",
    has_downloadable_artifact: true,
    ...partial,
  });

  it("downloads a run through the save dialog and never shows raw paths or timestamps", async () => {
    svc.listMyReportRuns.mockResolvedValue([
      run({}),
      run({
        id: 12,
        status: "failed",
        has_downloadable_artifact: false,
        artifact_path: null,
        error_message: "xlsx: disk full",
      }),
    ]);
    render(<AnalyticsHistoryPage />);
    await screen.findAllByText("templates.catalog.dashboard_summary.title");
    expect(screen.queryByText(/AppData/)).not.toBeInTheDocument();
    expect(screen.queryByText("2026-10-14T06:30:00Z")).not.toBeInTheDocument();
    expect(screen.getByText("runs.technicalDetails")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "runs.download" }));
    await waitFor(() => expect(svc.downloadReportRunArtifact).toHaveBeenCalledWith(11));
    expect(svc.saveExportedDocument).toHaveBeenCalledWith(DOC, "save.dialogTitle");
    expect(await screen.findByText("save.saved:dashboard-summary.pdf")).toBeInTheDocument();
  });
});
