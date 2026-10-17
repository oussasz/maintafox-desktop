import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { User } from "lucide-react";
import { useState } from "react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { KanbanBoard } from "@/components/kanban/KanbanBoard";
import type { KanbanCard, KanbanColumn, KanbanMoveOutcome } from "@/components/kanban/types";

const COLUMNS: KanbanColumn[] = [
  { id: "a", label: "Column A", tone: "info" },
  { id: "b", label: "Column B", tone: "progress" },
];

function renderBoard(ui: React.ReactElement) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

describe("KanbanBoard", () => {
  it("renders column headers and counters without icons", () => {
    const cards: KanbanCard[] = [
      { id: "1", columnId: "a", title: "Card 1" },
      { id: "2", columnId: "a", title: "Card 2" },
      { id: "3", columnId: "b", title: "Card 3" },
    ];
    const { container } = renderBoard(
      <KanbanBoard columns={COLUMNS} cards={cards} aria-label="Test board" />,
    );
    expect(screen.getByRole("region", { name: "Test board" })).toBeInTheDocument();
    expect(screen.getByText("Column A")).toBeInTheDocument();
    expect(screen.getByText("Column B")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("1")).toBeInTheDocument();
    expect(container.querySelectorAll("[data-column-id] svg").length).toBe(0);
  });

  it("renders the same empty label for every empty column", () => {
    renderBoard(<KanbanBoard columns={COLUMNS} cards={[]} emptyColumnLabel="Empty lane" />);
    const empties = screen.getAllByText("Empty lane");
    expect(empties).toHaveLength(2);
  });

  it("renders fixed card anatomy from shared slots", () => {
    const onCardClick = vi.fn();
    const cards: KanbanCard[] = [
      {
        id: "1",
        columnId: "a",
        code: "OT-0001",
        title: "Replace bearing",
        subtitle: "Pump A",
        description: "Vibration above threshold",
        badges: [{ label: "High", tone: "warning" }],
        meta: [{ label: "Ada", icon: User }],
        accent: "warning",
      },
    ];
    renderBoard(<KanbanBoard columns={COLUMNS} cards={cards} onCardClick={onCardClick} />);
    expect(screen.getByText("OT-0001")).toBeInTheDocument();
    expect(screen.getByText("Replace bearing")).toBeInTheDocument();
    expect(screen.getByText("Pump A")).toBeInTheDocument();
    expect(screen.getByText("Vibration above threshold")).toBeInTheDocument();
    expect(screen.getByText("High")).toBeInTheDocument();
    expect(screen.getByText("Ada")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "OT-0001: Replace bearing" }));
    expect(onCardClick).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }));
  });

  it("keeps code row free of trailing badges", () => {
    const cards: KanbanCard[] = [
      {
        id: "1",
        columnId: "a",
        code: "DI-0001",
        title: "Leak",
        badges: [{ label: "Critical", tone: "danger" }],
      },
    ];
    const { container } = renderBoard(<KanbanBoard columns={COLUMNS} cards={cards} />);
    const codeRow = container.querySelector(".font-mono")?.parentElement;
    expect(codeRow?.textContent).toBe("DI-0001");
    expect(screen.getByText("Critical")).toBeInTheDocument();
  });

  it("renders card actions without triggering card click", () => {
    const onCardClick = vi.fn();
    const onAction = vi.fn();
    const cards: KanbanCard[] = [
      {
        id: "1",
        columnId: "a",
        title: "With action",
        actions: [{ id: "approve", label: "Approve", onClick: onAction }],
      },
    ];
    renderBoard(<KanbanBoard columns={COLUMNS} cards={cards} onCardClick={onCardClick} />);
    fireEvent.click(screen.getByRole("button", { name: "Approve" }));
    expect(onAction).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }));
    expect(onCardClick).not.toHaveBeenCalled();
  });

  it("paginates with load more", () => {
    const cards: KanbanCard[] = Array.from({ length: 25 }, (_, i) => ({
      id: String(i),
      columnId: "a",
      title: `Card ${i}`,
    }));
    renderBoard(
      <KanbanBoard
        columns={COLUMNS}
        cards={cards}
        pageSize={10}
        loadMoreLabel={(n) => `Show ${n} more`}
      />,
    );
    expect(screen.getByText("Card 0")).toBeInTheDocument();
    expect(screen.queryByText("Card 10")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Show 15 more/i }));
    expect(screen.getByText("Card 10")).toBeInTheDocument();
  });

  it("calls onDrop when canDrop allows", () => {
    const onDrop = vi.fn();
    const canDrop = vi.fn(() => true);
    const cards: KanbanCard[] = [{ id: "1", columnId: "a", title: "Draggable" }];
    const { container } = renderBoard(
      <KanbanBoard columns={COLUMNS} cards={cards} enableDrag canDrop={canDrop} onDrop={onDrop} />,
    );
    const cardEl = screen.getByText("Draggable").closest("[draggable]") as HTMLElement;
    const colB = container.querySelector('[data-column-id="b"]') as HTMLElement;

    fireEvent.dragStart(cardEl, { dataTransfer: { setData: vi.fn(), effectAllowed: "move" } });
    fireEvent.dragOver(colB);
    fireEvent.drop(colB);

    expect(canDrop).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }), "a", "b");
    expect(onDrop).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }), "a", "b");
  });

  it("blocks onDrop when canDrop returns false", () => {
    const onDrop = vi.fn();
    const cards: KanbanCard[] = [{ id: "1", columnId: "a", title: "Blocked" }];
    const { container } = renderBoard(
      <KanbanBoard
        columns={COLUMNS}
        cards={cards}
        enableDrag
        canDrop={() => false}
        onDrop={onDrop}
      />,
    );
    const cardEl = screen.getByText("Blocked").closest("[draggable]") as HTMLElement;
    const colB = container.querySelector('[data-column-id="b"]') as HTMLElement;

    fireEvent.dragStart(cardEl, { dataTransfer: { setData: vi.fn(), effectAllowed: "move" } });
    fireEvent.drop(colB);
    expect(onDrop).not.toHaveBeenCalled();
  });

  it("produces identical chrome classes for DI-shaped and WO-shaped data", () => {
    const shape = (root: HTMLElement) =>
      Array.from(root.querySelectorAll<HTMLElement>("[data-column-id], [role='button']")).map(
        (el) => el.className,
      );

    const woBoard = renderBoard(
      <KanbanBoard
        columns={COLUMNS}
        cards={[
          {
            id: "1",
            columnId: "a",
            code: "OT-1",
            title: "WO title",
            subtitle: "Pump",
            badges: [{ label: "High", tone: "warning" }],
            meta: [{ label: "Ada", icon: User }],
            accent: "warning",
          },
        ]}
        emptyColumnLabel="—"
      />,
    );
    const woShape = shape(woBoard.container);
    woBoard.unmount();

    const diBoard = renderBoard(
      <KanbanBoard
        columns={COLUMNS}
        cards={[
          {
            id: "9",
            columnId: "a",
            code: "DI-9",
            title: "DI title",
            description: "Leak description",
            badges: [{ label: "Critical", tone: "danger" }],
            meta: [{ label: "15 Jun", icon: User }],
            accent: "danger",
          },
        ]}
        emptyColumnLabel="—"
      />,
    );
    expect(shape(diBoard.container)).toEqual(woShape);
  });
  it("uses the design-system tone classes shared with the list chips", () => {
    const { container } = renderBoard(
      <KanbanBoard
        columns={COLUMNS}
        cards={[
          {
            id: "1",
            columnId: "a",
            title: "Toned",
            badges: [{ label: "Overdue", tone: "danger" }],
            accent: "danger",
          },
        ]}
      />,
    );
    expect(screen.getByText("Overdue").closest("span.rounded-full")?.className).toContain(
      "bg-tone-danger-bg",
    );
    expect(container.querySelector(".bg-tone-danger-fg")).not.toBeNull();
    expect(container.querySelector(".bg-tone-info-fg")).not.toBeNull();
    // No private palette any more.
    expect(container.innerHTML).not.toMatch(/bg-(red|amber|orange|indigo|purple|emerald|teal)-\d/);
  });

  /** Board whose adapter moves the card only when the (fake) server accepts the transition. */
  function MovableBoard({
    canDrop,
    onDrop,
  }: {
    canDrop?: (card: KanbanCard, from: string, to: string) => boolean;
    onDrop?: (
      card: KanbanCard,
      from: string,
      to: string,
    ) => KanbanMoveOutcome | Promise<KanbanMoveOutcome>;
  }) {
    const [cards, setCards] = useState<KanbanCard[]>([{ id: "1", columnId: "a", title: "Mover" }]);
    return (
      <KanbanBoard
        columns={[...COLUMNS, { id: "c", label: "Column C", tone: "success" }]}
        cards={cards}
        enableDrag
        {...(canDrop ? { canDrop } : {})}
        onDrop={async (card, from, to) => {
          const outcome = await onDrop?.(card, from, to);
          const accepted =
            outcome === undefined ||
            outcome === true ||
            (typeof outcome === "object" && outcome.ok);
          if (accepted) {
            setCards((prev) => prev.map((c) => (c.id === card.id ? { ...c, columnId: to } : c)));
          }
          return outcome;
        }}
        aria-label="Movable"
      />
    );
  }

  it("moves a focused card with Alt+Arrow and keeps focus on it once accepted", async () => {
    const onDrop = vi.fn();
    const { container } = renderBoard(<MovableBoard onDrop={onDrop} />);
    const card = screen.getByRole("button", { name: "Mover" });
    card.focus();
    fireEvent.keyDown(card, { key: "ArrowRight", altKey: true });
    expect(onDrop).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }), "a", "b");
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("kanban.moved"));
    const moved = container.querySelector('[data-column-id="b"] [data-card-id="1"]');
    expect(moved).not.toBeNull();
    expect(document.activeElement).toBe(moved);
  });

  it("announces a refusal with its reason and does not claim the move", async () => {
    const onDrop = vi.fn(async () => ({ ok: false, reason: "Lines missing" }));
    const { container } = renderBoard(<MovableBoard onDrop={onDrop} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Mover" }), {
      key: "ArrowRight",
      altKey: true,
    });
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain("kanban.refusedReason"),
    );
    expect(screen.getByRole("status").textContent).not.toContain("kanban.moved");
    expect(container.querySelector('[data-column-id="a"] [data-card-id="1"]')).not.toBeNull();
  });

  it("announces a plain refusal (false) and a failed transition (rejected promise)", async () => {
    const refused = vi.fn(() => false);
    const first = renderBoard(<MovableBoard onDrop={refused} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Mover" }), {
      key: "ArrowRight",
      altKey: true,
    });
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("kanban.refused"));
    first.unmount();

    const failing = vi.fn(() => Promise.reject({ code: "VALIDATION_FAILED", message: "nope" }));
    renderBoard(<MovableBoard onDrop={failing} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Mover" }), {
      key: "ArrowRight",
      altKey: true,
    });
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain("kanban.refusedReason"),
    );
  });

  it("routes a drag and drop through the same result-aware path", async () => {
    const onDrop = vi.fn(() => ({ ok: false }));
    const { container } = renderBoard(<MovableBoard onDrop={onDrop} />);
    const cardEl = screen.getByText("Mover").closest("[draggable]") as HTMLElement;
    const colB = container.querySelector('[data-column-id="b"]') as HTMLElement;
    fireEvent.dragStart(cardEl, { dataTransfer: { setData: vi.fn(), effectAllowed: "move" } });
    fireEvent.drop(colB);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("kanban.refused"));
  });

  it("skips lanes canDrop refuses and announces when no lane accepts", async () => {
    const onDrop = vi.fn();
    const { container } = renderBoard(
      <MovableBoard onDrop={onDrop} canDrop={(_c, _f, to) => to === "c"} />,
    );
    const card = screen.getByRole("button", { name: "Mover" });
    fireEvent.keyDown(card, { key: "ArrowRight", altKey: true });
    expect(onDrop).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }), "a", "c");
    await waitFor(() =>
      expect(container.querySelector('[data-column-id="c"] [data-card-id="1"]')).not.toBeNull(),
    );

    onDrop.mockClear();
    fireEvent.keyDown(screen.getByRole("button", { name: "Mover" }), {
      key: "ArrowRight",
      altKey: true,
    });
    expect(onDrop).not.toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toContain("kanban.noTarget");
  });

  it("plain arrows and Enter keep their normal meaning", () => {
    const onDrop = vi.fn();
    renderBoard(<MovableBoard onDrop={onDrop} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Mover" }), { key: "ArrowRight" });
    expect(onDrop).not.toHaveBeenCalled();
  });

  it("offers a Move menu listing only the lanes that accept the card", async () => {
    const onDrop = vi.fn();
    renderBoard(<MovableBoard onDrop={onDrop} canDrop={(_c, _f, to) => to === "c"} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "kanban.moveCard" }), { key: "Enter" });
    expect(await screen.findByRole("menuitem", { name: "Column C" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Column B" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: "Column C" }));
    expect(onDrop).toHaveBeenCalledWith(expect.objectContaining({ id: "1" }), "a", "c");
  });

  it("offers no move affordance when the board does not allow moving", () => {
    renderBoard(
      <KanbanBoard columns={COLUMNS} cards={[{ id: "1", columnId: "a", title: "Fixed" }]} />,
    );
    expect(screen.queryByRole("button", { name: "kanban.moveCard" })).not.toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Fixed" })).not.toHaveAttribute("draggable", "true");
  });
});
