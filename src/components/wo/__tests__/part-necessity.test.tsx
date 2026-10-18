/**
 * Required / optional part lines: no preselected answer, a line is not added before the user
 * answers, optional lines are marked, and planning shows a short optional part as a warning
 * while a short required part blocks.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { PartNecessityChoice, PartOptionalChip } from "@/components/common/PartNecessityChoice";
import { ConflictReasonList } from "@/components/planning/ConflictReasonList";
import type { ConflictReason } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "articles" in opts ? `${key}:${String(opts["articles"])}` : key,
    i18n: { language: "en" },
  }),
}));

const exec = vi.hoisted(() => ({ addPart: vi.fn() }));
vi.mock("@/services/wo-execution-service", () => exec);
vi.mock("@/services/inventory-service", () => ({
  listInventoryArticles: vi.fn().mockResolvedValue([]),
  listInventoryStockBalances: vi.fn().mockResolvedValue([]),
}));
// Without the inventory right the dialog opens on a non-stock line (simplest path here).
vi.mock("@/lib/list-access", () => ({ useCanReadList: () => false }));
vi.mock("@/components/ui/list-access-note", () => ({ ListAccessNote: () => null }));

import { WoAddExecutionPartDialog } from "../WoAddExecutionPartDialog";

beforeEach(() => {
  vi.clearAllMocks();
  exec.addPart.mockResolvedValue({ id: 1 });
});

describe("PartNecessityChoice", () => {
  it("has no preselected answer and says the user must choose", () => {
    const onChange = vi.fn();
    render(<PartNecessityChoice value={null} onChange={onChange} partLabel="BRG-01" />);
    const group = screen.getByRole("radiogroup", { name: "partNecessity.legendFor" });
    const required = within(group).getByRole("radio", { name: "partNecessity.required" });
    const optional = within(group).getByRole("radio", { name: "partNecessity.optional" });
    expect(required).not.toBeChecked();
    expect(optional).not.toBeChecked();
    expect(screen.getByText("partNecessity.choose")).toBeInTheDocument();
    fireEvent.click(optional);
    expect(onChange).toHaveBeenCalledWith(false);
  });

  it("explains the effect of the answer", () => {
    const { rerender } = render(<PartNecessityChoice value onChange={vi.fn()} />);
    expect(screen.getByText("partNecessity.requiredHint")).toBeInTheDocument();
    rerender(<PartNecessityChoice value={false} onChange={vi.fn()} />);
    expect(screen.getByText("partNecessity.optionalHint")).toBeInTheDocument();
  });

  it("marks an optional line", () => {
    render(<PartOptionalChip />);
    expect(screen.getByText("partNecessity.optional")).toBeInTheDocument();
  });
});

describe("Adding a part during execution", () => {
  function open() {
    render(<WoAddExecutionPartDialog woId={5} open onOpenChange={vi.fn()} onAdded={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("execution.parts.add.label"), {
      target: { value: "Joint spécial" },
    });
  }

  it("refuses to add the line until it says required or optional", async () => {
    open();
    fireEvent.click(screen.getByRole("button", { name: /execution\.parts\.add\.save/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("common:partNecessity.choose");
    expect(exec.addPart).not.toHaveBeenCalled();
  });

  it("sends the answer with the line", async () => {
    open();
    fireEvent.click(screen.getByRole("radio", { name: "partNecessity.optional" }));
    fireEvent.click(screen.getByRole("button", { name: /execution\.parts\.add\.save/ }));
    await waitFor(() =>
      expect(exec.addPart).toHaveBeenCalledWith(
        expect.objectContaining({ wo_id: 5, article_ref: "Joint spécial", is_required: false }),
      ),
    );
  });
});

describe("Planning readiness", () => {
  it("shows a short optional part as a warning and a short required part as blocking", () => {
    const reasons = [
      {
        conflict_type: "missing_critical_part",
        reason_code: "PARTS_SHORT_OPTIONAL",
        severity: "warning",
        details_json: JSON.stringify({ articles: ["JOINT-2"], lines: [] }),
      },
      {
        conflict_type: "missing_critical_part",
        reason_code: "PARTS_SHORT",
        severity: "blocking",
        details_json: JSON.stringify({ articles: ["BRG-01"], lines: [] }),
      },
    ] as unknown as ConflictReason[];
    render(
      <MemoryRouter>
        <ConflictReasonList reasons={reasons} />
      </MemoryRouter>,
    );
    const items = screen.getAllByRole("listitem");
    // Blocking first, each with its own severity and its own text.
    expect(items[0]).toHaveAttribute("data-severity", "blocking");
    expect(items[0]).toHaveTextContent("reasons.PARTS_SHORT");
    expect(items[1]).toHaveAttribute("data-severity", "warning");
    expect(items[1]).toHaveTextContent("reasons.PARTS_SHORT_OPTIONAL");
  });
});
