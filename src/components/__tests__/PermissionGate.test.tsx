import { render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";

import { PersonnelExportMenu } from "@/components/personnel/PersonnelExportMenu";
import { PersonnelImportWizard } from "@/components/personnel/PersonnelImportWizard";
import { WorkforceReportPanel } from "@/components/personnel/WorkforceReportPanel";
import { PermissionGate } from "@/components/PermissionGate";
import { PermissionProvider } from "@/contexts/PermissionContext";

const mockGetMyPermissions = vi.fn();
const mockGetWorkforceSummaryReport = vi.fn();
const mockGetWorkforceKpiReport = vi.fn();
const mockGetWorkforceSkillsGapReport = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

vi.mock("@/services/personnel-service", () => ({
  exportWorkforceReportCsv: vi.fn(),
  createPersonnelImportBatch: vi.fn(),
  getPersonnelImportPreview: vi.fn(),
  applyPersonnelImportBatch: vi.fn(),
  getWorkforceSummaryReport: (...args: unknown[]) => mockGetWorkforceSummaryReport(...args),
  getWorkforceKpiReport: (...args: unknown[]) => mockGetWorkforceKpiReport(...args),
  getWorkforceSkillsGapReport: (...args: unknown[]) => mockGetWorkforceSkillsGapReport(...args),
}));

describe("PermissionGate", () => {
  const renderWithPermissionProvider = (ui: ReactNode) =>
    render(<PermissionProvider>{ui}</PermissionProvider>);

  beforeEach(() => {
    mockGetMyPermissions.mockReset();
    mockGetWorkforceSummaryReport.mockReset();
    mockGetWorkforceKpiReport.mockReset();
    mockGetWorkforceSkillsGapReport.mockReset();
    mockGetWorkforceSummaryReport.mockResolvedValue({
      total_personnel: 12,
      employment_breakdown: [],
      availability_breakdown: [],
    });
    mockGetWorkforceKpiReport.mockResolvedValue({
      avg_skills_per_person: 1.5,
      blocked_ratio: 0.1,
      assignment_density: 0.3,
      coverage_risk_ratio: 0.2,
    });
    mockGetWorkforceSkillsGapReport.mockResolvedValue([]);
  });

  it("renders children when user has the permission", async () => {
    mockGetMyPermissions.mockResolvedValue([
      {
        name: "eq.view",
        description: "",
        category: "equipment",
        is_dangerous: false,
        requires_step_up: false,
      },
    ]);

    renderWithPermissionProvider(
      <PermissionGate permission="eq.view" fallback={<span>no eq.view</span>}>
        <span data-testid="gate-content">has eq.view</span>
      </PermissionGate>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("gate-content")).toBeInTheDocument();
    });
    expect(screen.getByText("has eq.view")).toBeInTheDocument();
    expect(screen.queryByText("no eq.view")).not.toBeInTheDocument();
  });

  it("renders fallback when user lacks the permission", async () => {
    mockGetMyPermissions.mockResolvedValue([
      {
        name: "di.view",
        description: "",
        category: "intervention",
        is_dangerous: false,
        requires_step_up: false,
      },
    ]);

    renderWithPermissionProvider(
      <PermissionGate permission="eq.view" fallback={<span>no eq.view</span>}>
        <span data-testid="gate-content">has eq.view</span>
      </PermissionGate>,
    );

    await waitFor(() => {
      expect(screen.getByText("no eq.view")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("gate-content")).not.toBeInTheDocument();
  });

  it("renders nothing (null) during loading", () => {
    // Never resolve — stays in loading state
    mockGetMyPermissions.mockReturnValue(new Promise(() => {}));

    const { container } = renderWithPermissionProvider(
      <PermissionGate permission="eq.view">
        <span>should not appear</span>
      </PermissionGate>,
    );

    expect(container.innerHTML).toBe("");
  });

  it("renders nothing when no fallback and permission denied", async () => {
    mockGetMyPermissions.mockResolvedValue([]);

    const { container } = renderWithPermissionProvider(
      <PermissionGate permission="eq.view">
        <span>should not appear</span>
      </PermissionGate>,
    );

    await waitFor(() => {
      expect(container.innerHTML).toBe("");
    });
  });

  it("keeps personnel governance controls hidden without per permissions", async () => {
    mockGetMyPermissions.mockResolvedValue([]);

    const { container } = renderWithPermissionProvider(
      <>
        <PersonnelImportWizard />
        <PersonnelExportMenu />
        <WorkforceReportPanel />
      </>,
    );

    await waitFor(() => {
      expect(container.innerHTML).toBe("");
    });
  });

  it("shows personnel governance controls with per.manage and per.report", async () => {
    mockGetMyPermissions.mockResolvedValue([
      {
        name: "per.manage",
        description: "",
        category: "personnel",
        is_dangerous: false,
        requires_step_up: false,
      },
      {
        name: "per.report",
        description: "",
        category: "personnel",
        is_dangerous: false,
        requires_step_up: false,
      },
    ]);

    renderWithPermissionProvider(
      <>
        <PersonnelImportWizard />
        <PersonnelExportMenu />
        <WorkforceReportPanel />
      </>,
    );

    await waitFor(() => {
      expect(screen.getByText("import.action.openWizard")).toBeInTheDocument();
      expect(screen.getByText("reports.action.export")).toBeInTheDocument();
      expect(screen.getByText("reports.panel.title")).toBeInTheDocument();
    });
    expect(mockGetWorkforceSummaryReport).toHaveBeenCalledTimes(1);
    expect(mockGetWorkforceKpiReport).toHaveBeenCalledTimes(1);
    expect(mockGetWorkforceSkillsGapReport).toHaveBeenCalledTimes(1);
  });
});
