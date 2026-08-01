/**
 * ProcurementArchivePanel.tsx
 *
 * Collapsible SSOT for terminal procurement records (req / PO / repairable).
 * Same UX pattern as WoArchivePanel / DiArchivePanel.
 */

import type { ColumnDef } from "@tanstack/react-table";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { Badge } from "@/components/ui/badge";
import type { ProcurementRequisition, PurchaseOrder, RepairableOrder } from "@shared/ipc-types";

const REQ_TERMINAL = new Set(["CLOSED", "REJECTED", "CANCELLED"]);
const PO_TERMINAL = new Set(["RECEIVED_CLOSED", "CANCELLED"]);
const REPAIR_TERMINAL = new Set(["CLOSED", "SCRAPPED", "CANCELLED"]);

export interface ProcurementArchivePanelProps {
  requisitions: ProcurementRequisition[];
  purchaseOrders: PurchaseOrder[];
  repairables: RepairableOrder[];
  /** Bump after parent reload (props already update; kept for WO/DI parity). */
  refreshKey?: number;
  onRequisitionClick?: (req: ProcurementRequisition) => void;
  onPurchaseOrderClick?: (po: PurchaseOrder) => void;
  onRepairableClick?: (order: RepairableOrder) => void;
}

type ArchiveKind = "requisitions" | "purchaseOrders" | "repairables";

export function ProcurementArchivePanel({
  requisitions,
  purchaseOrders,
  repairables,
  refreshKey = 0,
  onRequisitionClick,
  onPurchaseOrderClick,
  onRepairableClick,
}: ProcurementArchivePanelProps) {
  const { t } = useTranslation("inventory");
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<ArchiveKind>("requisitions");

  const terminalReqs = useMemo(
    () => requisitions.filter((r) => REQ_TERMINAL.has(r.status)),
    [requisitions],
  );
  const terminalPos = useMemo(
    () => purchaseOrders.filter((p) => PO_TERMINAL.has(p.status)),
    [purchaseOrders],
  );
  const terminalRepairs = useMemo(
    () => repairables.filter((r) => REPAIR_TERMINAL.has(r.status)),
    [repairables],
  );

  const total = terminalReqs.length + terminalPos.length + terminalRepairs.length;

  const reqColumns: ColumnDef<ProcurementRequisition>[] = useMemo(
    () => [
      {
        accessorKey: "req_number",
        header: t("procurement.requisitions.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.req_number}</span>,
      },
      {
        accessorKey: "status",
        header: t("procurement.requisitions.columns.status"),
        cell: ({ row }) => (
          <Badge variant="outline" className="text-[10px]">
            {t(`procurement.statuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        accessorKey: "demand_source_type",
        header: t("procurement.requisitions.columns.source"),
        cell: ({ row }) => <span className="text-xs">{row.original.demand_source_type}</span>,
      },
      {
        accessorKey: "updated_at",
        header: t("procurement.requisitions.columns.updated"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.updated_at}</span>
        ),
      },
    ],
    [t],
  );

  const poColumns: ColumnDef<PurchaseOrder>[] = useMemo(
    () => [
      {
        accessorKey: "po_number",
        header: t("procurement.purchaseOrders.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.po_number}</span>,
      },
      {
        accessorKey: "status",
        header: t("procurement.purchaseOrders.columns.status"),
        cell: ({ row }) => (
          <Badge variant="outline" className="text-[10px]">
            {t(`procurement.statuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        id: "supplier",
        header: t("procurement.purchaseOrders.columns.supplier"),
        cell: ({ row }) => (
          <span className="text-xs">
            {row.original.supplier_company_name ?? row.original.supplier_name ?? "—"}
          </span>
        ),
      },
      {
        accessorKey: "updated_at",
        header: t("procurement.purchaseOrders.columns.updated"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.updated_at}</span>
        ),
      },
    ],
    [t],
  );

  const repairColumns: ColumnDef<RepairableOrder>[] = useMemo(
    () => [
      {
        accessorKey: "order_code",
        header: t("procurement.repairables.columns.order"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.order_code}</span>,
      },
      {
        accessorKey: "article_code",
        header: t("procurement.repairables.columns.article"),
        cell: ({ row }) => <span className="text-xs">{row.original.article_code}</span>,
      },
      {
        accessorKey: "status",
        header: t("procurement.repairables.columns.status"),
        cell: ({ row }) => (
          <Badge variant="outline" className="text-[10px]">
            {t(`procurement.repairableStatuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        accessorKey: "updated_at",
        header: t("procurement.repairables.columns.updated"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.updated_at}</span>
        ),
      },
    ],
    [t],
  );

  const sections: { id: ArchiveKind; label: string; count: number }[] = [
    {
      id: "requisitions",
      label: t("procurement.archive.requisitions"),
      count: terminalReqs.length,
    },
    {
      id: "purchaseOrders",
      label: t("procurement.archive.purchaseOrders"),
      count: terminalPos.length,
    },
    {
      id: "repairables",
      label: t("procurement.archive.repairables"),
      count: terminalRepairs.length,
    },
  ];

  return (
    <div className="rounded-md border">
      <button
        type="button"
        className="flex items-center gap-2 w-full px-4 py-2.5 hover:bg-muted/50 transition-colors text-left"
        onClick={() => setOpen(!open)}
        aria-expanded={open}
      >
        {open ? (
          <ChevronDown className="h-4 w-4 text-text-muted" />
        ) : (
          <ChevronRight className="h-4 w-4 text-text-muted" />
        )}
        <span className="text-sm font-medium text-text-primary">
          {t("procurement.archive.title")}
        </span>
        <Badge variant="secondary" className="text-xs">
          {total}
        </Badge>
      </button>

      {open && (
        <div key={refreshKey} className="px-4 pb-4 space-y-3">
          <div className="flex flex-wrap gap-1">
            {sections.map((s) => (
              <button
                key={s.id}
                type="button"
                onClick={() => setKind(s.id)}
                className={
                  kind === s.id
                    ? "rounded-md bg-primary px-2.5 py-1 text-xs text-primary-foreground"
                    : "rounded-md border px-2.5 py-1 text-xs hover:bg-muted"
                }
              >
                {`${s.label} (${s.count})`}
              </button>
            ))}
          </div>

          {kind === "requisitions" ? (
            <DataTable
              columns={reqColumns}
              data={terminalReqs}
              searchable
              pageSize={10}
              onRowClick={(row) => onRequisitionClick?.(row)}
            />
          ) : null}
          {kind === "purchaseOrders" ? (
            <DataTable
              columns={poColumns}
              data={terminalPos}
              searchable
              pageSize={10}
              onRowClick={(row) => onPurchaseOrderClick?.(row)}
            />
          ) : null}
          {kind === "repairables" ? (
            <DataTable
              columns={repairColumns}
              data={terminalRepairs}
              searchable
              pageSize={10}
              onRowClick={(row) => onRepairableClick?.(row)}
            />
          ) : null}
        </div>
      )}
    </div>
  );
}
