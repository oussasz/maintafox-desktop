/**
 * DiReviewTimeline — every review event names who decided it; system events say "System".
 */
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getDiReviewEvents } from "@/services/di-review-service";
import type { DiReviewEvent } from "@shared/ipc-types";

import { DiReviewTimeline } from "../DiReviewTimeline";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: { defaultValue?: string }) => opts?.defaultValue ?? key,
    i18n: { language: "en" },
  }),
}));
vi.mock("@/components/di/di-reference-labels", () => ({
  useDiDecisionReasonLabels: () => ({
    returnReasonLabel: (c: string) => c,
    deferralReasonLabel: (c: string) => c,
  }),
}));
vi.mock("@/services/di-review-service", () => ({ getDiReviewEvents: vi.fn() }));

function makeEvent(overrides: Partial<DiReviewEvent>): DiReviewEvent {
  return {
    id: 1,
    di_id: 1,
    event_type: "screened",
    actor_id: 7,
    actor_display_name: "Awa Diallo",
    acted_at: "2026-04-02T10:00:00Z",
    from_status: "submitted",
    to_status: "in_review",
    reason_code: null,
    notes: null,
    sla_target_hours: null,
    sla_deadline: null,
    step_up_used: false,
    ...overrides,
  };
}

describe("DiReviewTimeline", () => {
  beforeEach(() => {
    vi.mocked(getDiReviewEvents).mockReset();
  });

  it("shows the deciding person and labels system events", async () => {
    vi.mocked(getDiReviewEvents).mockResolvedValue([
      makeEvent({ id: 1 }),
      makeEvent({ id: 2, event_type: "sla_initialized", actor_id: null, actor_display_name: null }),
    ]);
    render(<DiReviewTimeline diId={1} />);
    expect(await screen.findByText(/Awa Diallo/)).toBeInTheDocument();
    expect(screen.getByText(/reviewTimeline\.systemActor/)).toBeInTheDocument();
  });
});
