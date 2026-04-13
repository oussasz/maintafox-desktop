import type { Table as TanStackTable } from "@tanstack/react-table";
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "../ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../ui/select";

// ─── Types ────────────────────────────────────────────────────────

interface DataTablePaginationProps<TData> {
  table: TanStackTable<TData>;
  /** Page size options shown in the selector */
  pageSizeOptions?: number[];
}

// ─── Component ────────────────────────────────────────────────────

const PAGE_SIZE_OPTIONS = [10, 20, 50] as const;

export function DataTablePagination<TData>({
  table,
  pageSizeOptions = [...PAGE_SIZE_OPTIONS],
}: DataTablePaginationProps<TData>) {
  const { t } = useTranslation("common");

  const currentPage = table.getState().pagination.pageIndex + 1;
  const totalPages = table.getPageCount();

  return (
    <div className="flex items-center justify-between px-2">
      {/* Page size selector */}
      <div className="flex items-center gap-2 text-sm text-text-muted">
        <span>{t("label.resultsPerPage")}</span>
        <Select
          value={String(table.getState().pagination.pageSize)}
          onValueChange={(value) => table.setPageSize(Number(value))}
        >
          <SelectTrigger className="h-8 w-[70px]">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {pageSizeOptions.map((size) => (
              <SelectItem key={size} value={String(size)}>
                {size}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Page indicator + navigation */}
      <div className="flex items-center gap-2">
        <span className="text-sm text-text-muted">
          {t("pagination.pageOf", { page: currentPage, total: totalPages })}
        </span>

        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon"
            className="h-8 w-8"
            onClick={() => table.setPageIndex(0)}
            disabled={!table.getCanPreviousPage()}
            aria-label={t("pagination.firstPage")}
          >
            <ChevronsLeft className="h-4 w-4" />
          </Button>
          <Button
            variant="outline"
            size="icon"
            className="h-8 w-8"
            onClick={() => table.previousPage()}
            disabled={!table.getCanPreviousPage()}
            aria-label={t("pagination.previousPage")}
          >
            <ChevronLeft className="h-4 w-4" />
          </Button>
          <Button
            variant="outline"
            size="icon"
            className="h-8 w-8"
            onClick={() => table.nextPage()}
            disabled={!table.getCanNextPage()}
            aria-label={t("pagination.nextPage")}
          >
            <ChevronRight className="h-4 w-4" />
          </Button>
          <Button
            variant="outline"
            size="icon"
            className="h-8 w-8"
            onClick={() => table.setPageIndex(table.getPageCount() - 1)}
            disabled={!table.getCanNextPage()}
            aria-label={t("pagination.lastPage")}
          >
            <ChevronsRight className="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
  );
}
