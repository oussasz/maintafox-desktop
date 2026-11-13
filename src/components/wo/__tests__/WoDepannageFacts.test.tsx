import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { WoDepannageFacts } from "@/components/wo/WoDepannageFacts";
import type { WorkOrder } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts && "value" in opts ? `${key}:${String(opts["value"])}` : key,
  i18n: { language: "fr" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
const svc = vi.hoisted(() => ({
  getDepannageTimes: vi.fn(),
  setWoBreakdownAt: vi.fn(),
  validateDepannageWo: vi.fn(),
}));
vi.mock("@/services/wo-service", () => svc);

const base = {
  id: 7,
  row_version: 3,
  status_code: "in_progress",
  is_depannage: true,
  breakdown_at: "2026-11-01T08:00:00Z",
  machine_stopped: true,
  requester_id: 1,
  validated_by_id: null,
  validated_at: null,
  validation_auto_approved: null,
} as unknown as WorkOrder;

beforeEach(() => {
  vi.clearAllMocks();
  svc.getDepannageTimes.mockResolvedValue({
    breakdown_at: "2026-11-01T08:00:00Z",
    downtime_start: "2026-11-01T08:00:00Z",
    restored_at: null,
    response_hours: 1.5,
    active_repair_hours: null,
    restore_hours: null,
    stopped_hours: null,
  });
});

describe("WoDepannageFacts", () => {
  it("shows a dépannage to validate and lets another approver validate it", async () => {
    const updated = { ...base, row_version: 4, validated_by_id: 2 } as unknown as WorkOrder;
    svc.validateDepannageWo.mockResolvedValue(updated);
    const onChanged = vi.fn();
    render(<WoDepannageFacts wo={base} canEdit canApprove userId={2} onChanged={onChanged} />);
    expect(screen.getByText("depannage.validation.badgeToValidate")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("wo-depannage-validate"));
    await waitFor(() =>
      expect(svc.validateDepannageWo).toHaveBeenCalledWith({ wo_id: 7, expected_row_version: 3 }),
    );
    expect(onChanged).toHaveBeenCalledWith(updated);
  });

  it("never offers the validation to the person who started the dépannage", () => {
    render(<WoDepannageFacts wo={base} canEdit canApprove userId={1} onChanged={vi.fn()} />);
    expect(screen.queryByTestId("wo-depannage-validate")).not.toBeInTheDocument();
    expect(screen.getByText("depannage.validation.notStarter")).toBeInTheDocument();
  });

  it("offers nothing without the approval right", () => {
    render(
      <WoDepannageFacts wo={base} canEdit canApprove={false} userId={2} onChanged={vi.fn()} />,
    );
    expect(screen.queryByTestId("wo-depannage-validate")).not.toBeInTheDocument();
    expect(screen.getByText("depannage.validation.pending")).toBeInTheDocument();
  });

  it("shows an automatic approval as such, never as a person's validation", () => {
    const auto = {
      ...base,
      status_code: "closed",
      validation_auto_approved: true,
    } as unknown as WorkOrder;
    render(<WoDepannageFacts wo={auto} canEdit canApprove userId={2} onChanged={vi.fn()} />);
    expect(screen.getByText("depannage.validation.autoApproved")).toBeInTheDocument();
    expect(screen.getByText("depannage.badge")).toBeInTheDocument();
  });

  it("shows the times of the dépannage", async () => {
    render(<WoDepannageFacts wo={base} canEdit canApprove userId={2} onChanged={vi.fn()} />);
    expect(await screen.findByTestId("wo-depannage-times")).toBeInTheDocument();
    expect(svc.getDepannageTimes).toHaveBeenCalledWith(7);
    expect(screen.getByText("depannage.times.hours:1.5")).toBeInTheDocument();
  });
});
