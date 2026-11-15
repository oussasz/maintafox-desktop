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

vi.mock("@/components/di/di-reference-labels", () => ({
  useDiReferenceLabels: () => ({ originLabel: (code: string) => `origin:${code}` }),
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
    // L1-28: named by what it covers, in the user's language (not the stored English name).
    expect(await screen.findByText("sla.scopeAllOrigins")).toBeInTheDocument();
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

  it("names a rule by its origin and criticality and says an inactive rule leaves the scale's targets (L1-28)", async () => {
    svc.listSlaRules.mockResolvedValue([
      { ...rule, origin_type: "operator", asset_criticality_class: "A", is_active: false },
    ]);
    render(<DiSlaRulesPanel open onClose={() => undefined} />);
    expect(await screen.findByText("origin:operator · sla.scopeCriticality")).toBeInTheDocument();
    expect(screen.getByText("sla.inactiveHint")).toBeInTheDocument();
    expect(screen.getByText("sla.escalationHint")).toBeInTheDocument();
  });
});
