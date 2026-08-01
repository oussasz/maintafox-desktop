import { fireEvent, render, screen } from "@testing-library/react";
import { User } from "lucide-react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { KanbanBoard } from "@/components/kanban/KanbanBoard";
import type { KanbanCard, KanbanColumn } from "@/components/kanban/types";

const COLUMNS: KanbanColumn[] = [
  { id: "a", label: "Column A", tone: "planning" },
  { id: "b", label: "Column B", tone: "executing" },
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
        badges: [{ label: "High", tone: "high" }],
        meta: [{ label: "Ada", icon: User }],
        accent: "high",
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
        badges: [{ label: "Critical", tone: "critical" }],
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
            badges: [{ label: "High", tone: "high" }],
            meta: [{ label: "Ada", icon: User }],
            accent: "high",
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
            badges: [{ label: "Critical", tone: "critical" }],
            meta: [{ label: "15 Jun", icon: User }],
            accent: "critical",
          },
        ]}
        emptyColumnLabel="—"
      />,
    );
    expect(shape(diBoard.container)).toEqual(woShape);
  });
});
