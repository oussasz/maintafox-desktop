import {
  type ColumnDef,
  type ColumnFiltersState,
  type SortingState,
  type Table as TanStackTable,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getPaginationRowModel,
  getSortedRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";

import { Input } from "../ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";

import { DataTablePagination } from "./DataTablePagination";

// ─── Types ────────────────────────────────────────────────────────

export interface DataTableProps<TData, TValue> {
  /** TanStack column definitions */
  columns: ColumnDef<TData, TValue>[];
  /** The data array to render */
  data: TData[];
  /** Enable the global search input above the table */
  searchable?: boolean;
  /** Search input placeholder override (default: i18n table.searchPlaceholder) */
  searchPlaceholder?: string;
  /** Initial page size (default: 10) */
  pageSize?: number;
  /** Callback when a row is clicked */
  onRowClick?: (row: TData) => void;
  /** Highlight selected row (e.g. registry two-pane selection) */
  isRowSelected?: (row: TData) => boolean;
  /** Show a loading skeleton instead of data */
  isLoading?: boolean;
  /** Number of skeleton rows to display when loading (default: 5) */
  skeletonRows?: number;
  /** Additional className for the outer wrapper */
  className?: string;
}

// ─── Sort indicator ───────────────────────────────────────────────

function SortIndicator({ direction }: { direction: false | "asc" | "desc" }) {
  if (direction === "asc") return <ArrowUp className="ml-1 inline h-4 w-4" />;
  if (direction === "desc") return <ArrowDown className="ml-1 inline h-4 w-4" />;
  return <ArrowUpDown className="ml-1 inline h-4 w-4 opacity-40" />;
}

// ─── Skeleton row ─────────────────────────────────────────────────

function SkeletonRow({ colCount }: { colCount: number }) {
  return (
    <TableRow>
      {Array.from({ length: colCount }, (_, i) => (
        <TableCell key={i}>
          <div className="h-4 w-3/4 animate-pulse rounded bg-muted" />
        </TableCell>
      ))}
    </TableRow>
  );
}

// ─── DataTable ────────────────────────────────────────────────────

/**
 * Renders a header or cell definition. A render function is called, not mounted as a component:
 * screens rebuild their column definitions when labels, catalogues, permissions or formatters
 * arrive, and `flexRender` would treat each new function as a new component type and re-mount
 * every row — a click landing during that swap was lost and focus jumped. Strings and real
 * components (memo / forwardRef objects) still go through `flexRender`.
 *
 * Rule for column authors: a cell or header that needs hooks returns a component element
 * (`cell: (ctx) => <StatusCell row={ctx.row.original} />`), never calls hooks itself — the
 * element keeps its own hook state when rows come and go. (Hooks in a plain `cell` function are
 * also refused by the rules-of-hooks lint.)
 */
function renderPart<TProps extends object>(
  part: string | ((props: TProps) => ReactNode) | ReactNode | undefined,
  props: TProps,
): ReactNode {
  if (typeof part === "function" && !(part.prototype && part.prototype.isReactComponent)) {
    return (part as (p: TProps) => ReactNode)(props);
  }
  return flexRender(part as Parameters<typeof flexRender>[0], props);
}

export function DataTable<TData, TValue>({
  columns,
  data,
  searchable = false,
  searchPlaceholder,
  pageSize = 10,
  onRowClick,
  isRowSelected,
  isLoading = false,
  skeletonRows = 5,
  className,
}: DataTableProps<TData, TValue>) {
  const { t } = useTranslation("common");

  const [sorting, setSorting] = useState<SortingState>([]);
  const [columnFilters, setColumnFilters] = useState<ColumnFiltersState>([]);
  const [globalFilter, setGlobalFilter] = useState("");

  const table: TanStackTable<TData> = useReactTable({
    data,
    columns,
    state: { sorting, columnFilters, globalFilter },
    onSortingChange: setSorting,
    onColumnFiltersChange: setColumnFilters,
    onGlobalFilterChange: setGlobalFilter,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    initialState: {
      pagination: { pageSize },
    },
  });

  return (
    <div className={cn("space-y-3", className)}>
      {/* Global search */}
      {searchable && (
        <div className="flex items-center">
          <Input
            placeholder={searchPlaceholder ?? t("table.searchPlaceholder")}
            value={globalFilter}
            onChange={(e) => setGlobalFilter(e.target.value)}
            className="max-w-sm"
          />
        </div>
      )}

      {/* Table */}
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
                  const canSort = header.column.getCanSort();
                  return (
                    <TableHead
                      key={header.id}
                      className={cn(canSort && "cursor-pointer select-none")}
                      onClick={canSort ? header.column.getToggleSortingHandler() : undefined}
                      aria-sort={
                        header.column.getIsSorted() === "asc"
                          ? "ascending"
                          : header.column.getIsSorted() === "desc"
                            ? "descending"
                            : "none"
                      }
                    >
                      {header.isPlaceholder ? null : (
                        <span className="inline-flex items-center">
                          {renderPart(header.column.columnDef.header, header.getContext())}
                          {canSort && <SortIndicator direction={header.column.getIsSorted()} />}
                        </span>
                      )}
                    </TableHead>
                  );
                })}
              </TableRow>
            ))}
          </TableHeader>

          <TableBody>
            {isLoading ? (
              Array.from({ length: skeletonRows }, (_, i) => (
                <SkeletonRow key={i} colCount={columns.length} />
              ))
            ) : table.getRowModel().rows.length > 0 ? (
              table.getRowModel().rows.map((row) => {
                const selected = isRowSelected?.(row.original) ?? row.getIsSelected();
                return (
                  <TableRow
                    key={row.id}
                    data-state={selected ? "selected" : undefined}
                    className={cn(onRowClick && "cursor-pointer", selected && "bg-muted")}
                    onClick={onRowClick ? () => onRowClick(row.original) : undefined}
                  >
                    {row.getVisibleCells().map((cell) => (
                      <TableCell key={cell.id}>
                        {renderPart(cell.column.columnDef.cell, cell.getContext())}
                      </TableCell>
                    ))}
                  </TableRow>
                );
              })
            ) : (
              <TableRow>
                <TableCell colSpan={columns.length} className="h-24 text-center text-text-muted">
                  {t("label.noResults")}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      {/* Pagination */}
      {!isLoading && <DataTablePagination table={table} />}
    </div>
  );
}
