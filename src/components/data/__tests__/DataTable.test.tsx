// src/components/data/__tests__/DataTable.test.tsx
// V2 — Supervisor verification: confirms DataTable renders rows, pagination, headers,
// sorting, and empty-state message.

import assert from "node:assert";

import type { ColumnDef } from "@tanstack/react-table";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

import { DataTable } from "../DataTable";

// ── i18n mock ───────────────────────────────────────────────────
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

// ── Sample data ─────────────────────────────────────────────────

interface SampleRow {
  id: number;
  name: string;
  status: string;
}

function generateRows(count: number): SampleRow[] {
  return Array.from({ length: count }, (_, i) => ({
    id: i + 1,
    name: `Item ${String(i + 1).padStart(3, "0")}`,
    status: i % 2 === 0 ? "active" : "inactive",
  }));
}

const columns: ColumnDef<SampleRow, unknown>[] = [
  { accessorKey: "id", header: "ID" },
  { accessorKey: "name", header: "Name" },
  { accessorKey: "status", header: "Status" },
];

// ── Tests ───────────────────────────────────────────────────────

describe("DataTable — V2 Supervisor Verification", () => {
  it("renders column headers", () => {
    render(<DataTable columns={columns} data={generateRows(5)} />);
    expect(screen.getByText("ID")).toBeInTheDocument();
    expect(screen.getByText("Name")).toBeInTheDocument();
    expect(screen.getByText("Status")).toBeInTheDocument();
  });

  it("renders 10 rows from 50 items (default page size)", () => {
    render(<DataTable columns={columns} data={generateRows(50)} />);
    // Table body should have exactly 10 data rows on page 1
    const rows = screen.getAllByRole("row");
    // 1 header row + 10 data rows = 11
    expect(rows).toHaveLength(11);
  });

  it("shows pagination with 5 pages for 50 items at pageSize=10", () => {
    render(<DataTable columns={columns} data={generateRows(50)} />);
    expect(screen.getByText("Page 1 of 5")).toBeInTheDocument();
  });

  it("navigates to next page", () => {
    render(<DataTable columns={columns} data={generateRows(50)} />);

    // Page 1: first row should be Item 001
    expect(screen.getByText("Item 001")).toBeInTheDocument();

    // Click next page
    const nextBtn = screen.getByRole("button", { name: "Next page" });
    fireEvent.click(nextBtn);

    // Page 2: should show Item 011
    expect(screen.getByText("Item 011")).toBeInTheDocument();
    expect(screen.getByText("Page 2 of 5")).toBeInTheDocument();
  });

  it("sorts column when header is clicked", () => {
    const data = generateRows(5);
    render(<DataTable columns={columns} data={data} />);

    // Initial order: Item 001, Item 002, ...
    const tbody = screen.getAllByRole("row").slice(1); // skip header
    const firstRow = tbody[0];
    assert(firstRow, "Expected at least one data row");
    expect(within(firstRow).getByText("Item 001")).toBeInTheDocument();

    // Click "Name" header to sort ascending (already asc by default data, toggle to desc)
    const nameHeader = screen.getByText("Name");
    fireEvent.click(nameHeader); // asc
    fireEvent.click(nameHeader); // desc

    const rowsAfterSort = screen.getAllByRole("row").slice(1);
    const firstSortedRow = rowsAfterSort[0];
    assert(firstSortedRow, "Expected at least one sorted row");
    expect(within(firstSortedRow).getByText("Item 005")).toBeInTheDocument();
  });

  it("displays empty state message when data is empty", () => {
    render(<DataTable columns={columns} data={[]} />);
    expect(screen.getByText("No results found")).toBeInTheDocument();
  });

  it("shows global search input when searchable=true", () => {
    render(<DataTable columns={columns} data={generateRows(5)} searchable />);
    expect(screen.getByPlaceholderText("Search...")).toBeInTheDocument();
  });

  it("filters rows via global search", () => {
    render(<DataTable columns={columns} data={generateRows(20)} searchable />);

    const searchInput = screen.getByPlaceholderText("Search...");
    fireEvent.change(searchInput, { target: { value: "Item 015" } });

    // Only 1 row should match
    const rows = screen.getAllByRole("row");
    expect(rows).toHaveLength(2); // 1 header + 1 data
    expect(screen.getByText("Item 015")).toBeInTheDocument();
  });

  it("shows skeleton rows when isLoading=true", () => {
    const { container } = render(
      <DataTable columns={columns} data={[]} isLoading skeletonRows={3} />,
    );
    const pulseElements = container.querySelectorAll(".animate-pulse");
    // 3 skeleton rows × 3 columns = 9 pulse elements
    expect(pulseElements).toHaveLength(9);
  });
});
