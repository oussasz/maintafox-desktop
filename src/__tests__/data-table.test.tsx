// src/__tests__/data-table.test.tsx
// Phase 2 · SP00-F04 · S1 — Integration tests for DataTable covering
// rendering, empty state, pagination, sorting, and accessibility attributes.

import type { ColumnDef } from "@tanstack/react-table";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { DataTable } from "@/components/data";

// ── i18n mock (same contract as the component-level tests) ──────

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const map: Record<string, string> = {
        "table.searchPlaceholder": "Search...",
        "label.noResults": "No results found",
        "label.resultsPerPage": "Results per page",
        "pagination.pageOf": `Page ${opts?.["page"] ?? ""} of ${opts?.["total"] ?? ""}`,
        "pagination.firstPage": "First page",
        "pagination.lastPage": "Last page",
        "pagination.nextPage": "Next page",
        "pagination.previousPage": "Previous page",
      };
      return map[key] ?? key;
    },
    i18n: { language: "en" },
  }),
}));

// ── Test data ───────────────────────────────────────────────────

interface Equipment {
  id: number;
  code: string;
  designation: string;
  status: string;
}

function makeEquipment(count: number): Equipment[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    code: `EQ-${String(i + 1).padStart(4, "0")}`,
    designation: `Equipment ${i + 1}`,
    status: i % 3 === 0 ? "en_service" : i % 3 === 1 ? "hors_service" : "en_attente",
  }));
}

const columns: ColumnDef<Equipment, unknown>[] = [
  { accessorKey: "id", header: "ID", enableSorting: true },
  { accessorKey: "code", header: "Code", enableSorting: true },
  { accessorKey: "designation", header: "Designation", enableSorting: true },
  { accessorKey: "status", header: "Statut", enableSorting: false },
];

// ── Tests ───────────────────────────────────────────────────────

describe("DataTable — Integration (SP00-F04)", () => {
  it("renders the correct number of data rows", () => {
    render(<DataTable columns={columns} data={makeEquipment(7)} />);
    // 1 header row + 7 data rows = 8
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(8);
  });

  it("renders all column headers", () => {
    render(<DataTable columns={columns} data={makeEquipment(1)} />);
    expect(screen.getByText("ID")).toBeInTheDocument();
    expect(screen.getByText("Code")).toBeInTheDocument();
    expect(screen.getByText("Designation")).toBeInTheDocument();
    expect(screen.getByText("Statut")).toBeInTheDocument();
  });

  it("shows empty-state message when data is []", () => {
    render(<DataTable columns={columns} data={[]} />);
    expect(screen.getByText("No results found")).toBeInTheDocument();
    // Should still render header row + 1 empty-state row
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(2);
  });

  it("shows correct page count for custom pageSize", () => {
    // 25 items with pageSize=5 → 5 pages
    render(<DataTable columns={columns} data={makeEquipment(25)} pageSize={5} />);
    expect(screen.getByText("Page 1 of 5")).toBeInTheDocument();

    // Only 5 data rows should be rendered
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(6); // 1 header + 5 data
  });

  it("toggles column sort order on header click", () => {
    render(<DataTable columns={columns} data={makeEquipment(5)} />);

    const codeHeader = screen.getByText("Code");

    // Click once → asc sort
    fireEvent.click(codeHeader);
    const rowsAsc = screen.getAllByRole("row").slice(1);
    expect(within(rowsAsc.at(0) as HTMLElement).getByText("EQ-0001")).toBeInTheDocument();

    // Click again → desc sort
    fireEvent.click(codeHeader);
    const rowsDesc = screen.getAllByRole("row").slice(1);
    expect(within(rowsDesc.at(0) as HTMLElement).getByText("EQ-0005")).toBeInTheDocument();
  });

  it("applies aria-sort attributes on sortable columns", () => {
    render(<DataTable columns={columns} data={makeEquipment(3)} />);

    // All sortable columnheaders should start with aria-sort="none"
    const headerRow = screen.getAllByRole("row").at(0) as HTMLElement;
    const headers = within(headerRow).getAllByRole("columnheader");

    // ID, Code, Designation are sortable → should have aria-sort
    const sortableHeaders = headers.filter((h) => h.getAttribute("aria-sort") !== null);
    expect(sortableHeaders.length).toBeGreaterThanOrEqual(3);

    for (const h of sortableHeaders) {
      expect(h).toHaveAttribute("aria-sort", "none");
    }

    // Click "Code" → ascending
    fireEvent.click(screen.getByText("Code"));
    const codeHeader = headers.find((h) => h.textContent?.includes("Code"));
    expect(codeHeader).toHaveAttribute("aria-sort", "ascending");
  });

  it("fires onRowClick with the correct row data", () => {
    const handleClick = vi.fn();
    render(<DataTable columns={columns} data={makeEquipment(3)} onRowClick={handleClick} />);

    // Click the second data row (Equipment 2)
    const rows = screen.getAllByRole("row").slice(1); // skip header
    fireEvent.click(rows.at(1) as HTMLElement);

    expect(handleClick).toHaveBeenCalledTimes(1);
    expect(handleClick).toHaveBeenCalledWith(expect.objectContaining({ id: 2, code: "EQ-0002" }));
  });

  it("paginates correctly to the last page", () => {
    // 12 items at pageSize=5 → 3 pages, last page has 2 items
    render(<DataTable columns={columns} data={makeEquipment(12)} pageSize={5} />);

    expect(screen.getByText("Page 1 of 3")).toBeInTheDocument();

    // Navigate to last page
    const lastBtn = screen.getByRole("button", { name: "Last page" });
    fireEvent.click(lastBtn);

    expect(screen.getByText("Page 3 of 3")).toBeInTheDocument();

    // Should display only 2 data rows on the last page
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(3); // 1 header + 2 data
  });
});
