import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { DiSlaRulesPanel } from "@/components/di/DiSlaRulesPanel";
import type { DiSlaRule } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

vi.mock("@/components/common/urgency/UrgencyBadge", () => ({
  UrgencyBadge: () => <span>urgency</span>,
}));

const svc = vi.hoisted(() => ({
  listSlaRules: vi.fn(),
  updateSlaRule: vi.fn(),
}));
vi.mock("@/services/di-conversion-service", () => svc);

const rule = {
  id: 1,
  name: "Standard",
  urgency_level: "normal",
  origin_type: null,
  asset_criticality_class: null,
  target_response_hours: 4,
  target_resolution_hours: 24,
  escalation_threshold_hours: 12,
  is_active: true,
} as unknown as DiSlaRule;

describe("DiSlaRulesPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows a retryable error instead of an empty table when the rules cannot be read", async () => {
    svc.listSlaRules.mockRejectedValueOnce(new Error("boom")).mockResolvedValueOnce([rule]);
    render(<DiSlaRulesPanel open onClose={() => undefined} />);
    const retry = await screen.findByRole("button", { name: /retry|réessayer/i });
    fireEvent.click(retry);
    expect(await screen.findByText("Standard")).toBeInTheDocument();
  });

  it("refuses an emptied hours field instead of saving 0", async () => {
    svc.listSlaRules.mockResolvedValue([rule]);
    render(<DiSlaRulesPanel open onClose={() => undefined} />);
    const [response] = (await screen.findAllByRole("spinbutton")) as HTMLInputElement[];
    fireEvent.change(response as HTMLInputElement, { target: { value: "" } });
    expect((response as HTMLInputElement).value).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "sla.save" }));
    expect(await screen.findByText("sla.invalidHours")).toBeInTheDocument();
    expect(svc.updateSlaRule).not.toHaveBeenCalled();
  });

  it("saves whole hours", async () => {
    svc.listSlaRules.mockResolvedValue([rule]);
    svc.updateSlaRule.mockResolvedValue(undefined);
    render(<DiSlaRulesPanel open onClose={() => undefined} />);
    const [response] = (await screen.findAllByRole("spinbutton")) as HTMLInputElement[];
    fireEvent.change(response as HTMLInputElement, { target: { value: "6" } });
    fireEvent.click(screen.getByRole("button", { name: "sla.save" }));
    await waitFor(() =>
      expect(svc.updateSlaRule).toHaveBeenCalledWith(
        expect.objectContaining({ id: 1, target_response_hours: 6, target_resolution_hours: 24 }),
      ),
    );
  });
});
