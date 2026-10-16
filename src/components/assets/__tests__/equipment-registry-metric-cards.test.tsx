import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { EquipmentRegistryMetricCards } from "@/components/assets/EquipmentRegistryMetricCards";
import type { EquipmentRegistryMetricsPayload } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "active" in opts ? `${key}:${String(opts["active"])}/${String(opts["down"])}` : key,
    i18n: { language: "fr" },
  }),
}));

const svc = vi.hoisted(() => ({ getEquipmentRegistryMetrics: vi.fn() }));
vi.mock("@/services/asset-search-service", () => svc);

const PAYLOAD: EquipmentRegistryMetricsPayload = {
  scoped_total: 12,
  cells: [
    {
      semantic_key: "EQUIP_STATUS_SUMMARY",
      catalog_id: "KPI-047",
      value: 2,
      available: true,
      value_kind: "count",
      trend_sense: "up-bad",
      active_count: 9,
      down_count: 2,
    },
    {
      semantic_key: "EQUIP_OPEN_WORK",
      catalog_id: "KPI-050",
      value: 1234,
      available: true,
      value_kind: "count",
      trend_sense: "up-bad",
      series: [1, 2],
    },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
});

describe("EquipmentRegistryMetricCards", () => {
  it("shows current counts with the app's number format and no invented trend", async () => {
    svc.getEquipmentRegistryMetrics.mockResolvedValue(PAYLOAD);
    render(<EquipmentRegistryMetricCards />);
    expect(await screen.findByText("metrics.contextStatusDetail:9/2")).toBeInTheDocument();
    // French grouping (narrow no-break space), never the OS locale's "1,234".
    expect(screen.getByText(/1\s234/)).toBeInTheDocument();
    expect(screen.queryByText("metrics.stable")).not.toBeInTheDocument();
    expect(screen.queryByText(/%$/)).not.toBeInTheDocument();
  });

  it("shows the failure with a retry instead of empty tiles", async () => {
    svc.getEquipmentRegistryMetrics.mockRejectedValueOnce({
      code: "DATABASE_ERROR",
      message: "SQLITE_BUSY",
    });
    render(<EquipmentRegistryMetricCards />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByText("metrics.loadError")).toBeInTheDocument();

    svc.getEquipmentRegistryMetrics.mockResolvedValueOnce(PAYLOAD);
    fireEvent.click(screen.getByRole("button"));
    expect(await screen.findByText("metrics.contextStatusDetail:9/2")).toBeInTheDocument();
    expect(svc.getEquipmentRegistryMetrics).toHaveBeenCalledTimes(2);
  });
});
