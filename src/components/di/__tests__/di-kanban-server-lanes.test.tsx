/**
 * The request board reads each lane from the server with the list's filters and queue: the lane
 * count is the server total and "load more" reads the lane's next page.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { DiKanbanBoard } from "@/components/di/DiKanbanBoard";
import type { DiListFilter, InterventionRequest } from "@shared/ipc-types";

const i18nMock = vi.hoisted(() => ({
  t: (key: string) => key,
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));
vi.mock("@/components/di/di-reference-labels", () => ({
  useDiReferenceLabels: () => ({ originLabel: (code: string) => code }),
}));
vi.mock("@/components/common/urgency/use-urgency-scale", () => ({
  useUrgencyScale: () => ({ labelOf: (code: string) => code }),
  urgencyTone: () => undefined,
}));

const svc = vi.hoisted(() => ({ listDis: vi.fn() }));
vi.mock("@/services/di-service", () => svc);

function requests(from: number, n: number): InterventionRequest[] {
  return Array.from({ length: n }, (_, i) => ({
    id: from + i,
    code: `DI-${from + i}`,
    title: `Leak ${from + i}`,
    description: "",
    status: "submitted",
    submitted_at: "2026-10-01T08:00:00Z",
    origin_type: "operator",
    validated_urgency: null,
    reported_urgency: null,
    sla_state: null,
    safety_flag: false,
    converted_to_wo_code: null,
    converted_to_wo_id: null,
    converted_to_wo_title: null,
  })) as unknown as InterventionRequest[];
}

describe("DiKanbanBoard server lanes", () => {
  it("counts the lane's server total and loads its next page with the list's queue", async () => {
    svc.listDis.mockImplementation(({ status, offset }: DiListFilter) =>
      Promise.resolve(
        status?.includes("submitted")
          ? { items: offset === 0 ? requests(1, 20) : requests(21, 3), total: 23 }
          : { items: [], total: 0 },
      ),
    );
    render(
      <MemoryRouter>
        <DiKanbanBoard
          filter={{ queue: "to_review", limit: 50, offset: 0 }}
          onCardClick={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(await screen.findByText("Leak 20")).toBeInTheDocument();
    expect(screen.getByText("23")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /kanban\.showMore/ }));
    expect(await screen.findByText("Leak 23")).toBeInTheDocument();
    await waitFor(() =>
      expect(svc.listDis).toHaveBeenLastCalledWith(
        expect.objectContaining({ queue: "to_review", status: ["submitted"], offset: 20 }),
      ),
    );
  });
});
