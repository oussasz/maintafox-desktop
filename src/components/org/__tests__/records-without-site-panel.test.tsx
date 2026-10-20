/**
 * "Records without a site" (CMMS Advisor decision Q1): the administrator selects old records
 * without a site; the site is pre-filled with their equipment's site only as a suggestion (none
 * when the selected records' equipment are on different sites), a reason is required, and the
 * confirmed batch is sent with that site and reason. A failed load is an error with retry, never an
 * empty list.
 */

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  RecordsWithoutSitePanel,
  commonSuggestion,
} from "@/components/org/RecordsWithoutSitePanel";
import type { NoSiteRecord, RecordsWithoutSite } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "record" in opts ? `${key} ${String(opts["record"])}` : key,
  }),
}));

vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => action(),
    StepUpDialogElement: null,
  }),
  isStepUpCancelled: () => false,
}));

vi.mock("@/hooks/use-formatters", () => ({
  useFormatters: () => ({
    formatDate: (v: unknown) => String(v ?? ""),
    formatDateTime: (v: unknown) => String(v ?? ""),
  }),
}));

vi.mock("@/store/app-toast-store", () => ({ pushAppToast: vi.fn() }));

const svc = vi.hoisted(() => ({
  listRecordsWithoutSite: vi.fn(),
  assignSiteToRecords: vi.fn(),
}));
vi.mock("@/services/site-assignment-service", () => ({
  listRecordsWithoutSite: svc.listRecordsWithoutSite,
  assignSiteToRecords: svc.assignSiteToRecords,
}));

function record(id: number, code: string, suggested: number | null): NoSiteRecord {
  return {
    kind: "work_order",
    id,
    code,
    title: `Old order ${code}`,
    status_code: "closed",
    created_at: "2025-03-01T08:00:00Z",
    window_start: null,
    window_end: null,
    equipment_id: 100 + id,
    equipment_code: `EQ-${id}`,
    equipment_name: `Pump ${id}`,
    suggested_node_id: suggested,
    suggested_node_name: suggested == null ? null : `Site ${suggested}`,
    to_review: true,
  };
}

const counts = {
  work_orders: 3,
  requests: 1,
  planning_windows_to_review: 0,
  planning_windows_plant_wide: 2,
};

function data(records: NoSiteRecord[]): RecordsWithoutSite {
  return {
    kind: "work_order",
    counts,
    records,
    listed_limit: 500,
    max_batch: 500,
    reason_min_chars: 10,
    nodes: [
      { id: 7, parent_id: null, code: "S7", name: "Site 7", type_label: "Site", depth: 1 },
      { id: 8, parent_id: null, code: "S8", name: "Site 8", type_label: "Site", depth: 1 },
    ],
  };
}

beforeEach(() => {
  svc.listRecordsWithoutSite.mockReset();
  svc.assignSiteToRecords.mockReset();
});

describe("commonSuggestion", () => {
  it("offers the site shared by every selected record, and nothing otherwise", () => {
    const rows = [record(1, "WO-1", 7), record(2, "WO-2", 7), record(3, "WO-3", 8)];
    expect(commonSuggestion(rows, new Set([1, 2]))).toBe(7);
    expect(commonSuggestion(rows, new Set([1, 3]))).toBeNull();
    expect(commonSuggestion([record(4, "WO-4", null)], new Set([4]))).toBeNull();
    expect(commonSuggestion(rows, new Set())).toBeNull();
  });
});

describe("RecordsWithoutSitePanel", () => {
  it("sends the selected records with the suggested site once a reason is given", async () => {
    const rows = [record(1, "WO-1", 7), record(2, "WO-2", 7), record(3, "WO-3", 8)];
    svc.listRecordsWithoutSite.mockResolvedValue(data(rows));
    const after = { ...counts, work_orders: 2 };
    svc.assignSiteToRecords.mockResolvedValueOnce({
      kind: "work_order",
      org_node_id: 7,
      assigned: [1],
      counts: after,
    });
    const onChanged = vi.fn();

    render(<RecordsWithoutSitePanel open onOpenChange={vi.fn()} onChanged={onChanged} />);

    fireEvent.click(await screen.findByLabelText("org:noSite.selectRow WO-1"));
    const confirm = screen.getByRole("button", { name: "org:noSite.confirm" });
    expect(confirm).toBeDisabled();

    fireEvent.change(screen.getByLabelText("org:noSite.reason"), {
      target: { value: "  Old order of the pump house  " },
    });
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);

    await waitFor(() =>
      expect(svc.assignSiteToRecords).toHaveBeenCalledWith({
        kind: "work_order",
        record_ids: [1],
        org_node_id: 7,
        reason: "Old order of the pump house",
      }),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalledWith(after));
    expect(svc.listRecordsWithoutSite).toHaveBeenCalledTimes(2);
  });

  it("does not pick a site when the selected records' equipment are on different sites", async () => {
    const rows = [record(1, "WO-1", 7), record(3, "WO-3", 8)];
    svc.listRecordsWithoutSite.mockResolvedValue(data(rows));

    render(<RecordsWithoutSitePanel open onOpenChange={vi.fn()} />);

    fireEvent.click(await screen.findByLabelText("org:noSite.selectRow WO-1"));
    fireEvent.click(screen.getByLabelText("org:noSite.selectRow WO-3"));
    fireEvent.change(screen.getByLabelText("org:noSite.reason"), {
      target: { value: "Old orders of the pump house" },
    });

    expect(screen.getByRole("button", { name: "org:noSite.confirm" })).toBeDisabled();
    expect(screen.getByText("org:noSite.siteHintNone")).toBeInTheDocument();
    expect(svc.assignSiteToRecords).not.toHaveBeenCalled();
  });

  it("shows a failed load as an error with retry, never as an empty list", async () => {
    svc.listRecordsWithoutSite.mockRejectedValueOnce(new Error("database locked"));

    render(<RecordsWithoutSitePanel open onOpenChange={vi.fn()} />);

    expect(await screen.findByText("org:noSite.loadFailed")).toBeInTheDocument();
    expect(screen.queryByText("org:noSite.emptyTitle")).not.toBeInTheDocument();
  });
});
