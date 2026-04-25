import type { ColumnDef } from "@tanstack/react-table";
import { Columns3, List } from "lucide-react";
import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import {
  createInventoryProcurementRequisition,
  createInventoryPurchaseOrderFromRequisition,
  createInventoryRepairableOrder,
  listInventoryArticles,
  listInventoryLocations,
  listInventoryStockBalances,
  listInventoryProcurementRequisitionLines,
  listInventoryProcurementRequisitions,
  listInventoryProcurementSuppliers,
  listInventoryPurchaseOrderLines,
  listInventoryPurchaseOrders,
  listInventoryRepairableOrders,
  listInventoryStateEvents,
  receiveInventoryPurchaseOrderGoods,
  transitionInventoryProcurementRequisition,
  transitionInventoryPurchaseOrder,
  transitionInventoryRepairableOrder,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  InventoryArticle,
  InventoryStockBalance,
  InventoryStateEvent,
  ProcurementRequisition,
  ProcurementRequisitionLine,
  ProcurementSupplier,
  PurchaseOrder,
  PurchaseOrderLine,
  RepairableOrder,
  StockLocation,
} from "@shared/ipc-types";

export type ProcurementRepairablePanelHandle = {
  openCreateRequisition: () => void;
  openCreatePo: () => void;
  openReceiveGoods: () => void;
  openRepairable: () => void;
};

type ProcurementRepairablePanelProps = {
  /** When set, list/kanban is controlled by the parent (e.g. Inventory page header). */
  viewMode?: "list" | "kanban";
  onViewModeChange?: (mode: "list" | "kanban") => void;
};

type SubTab = "requisitions" | "purchase-orders" | "repairables" | "lifecycle";

export const ProcurementRepairablePanel = forwardRef<
  ProcurementRepairablePanelHandle,
  ProcurementRepairablePanelProps
>(function ProcurementRepairablePanel(props, ref) {
  const { viewMode: viewModeProp, onViewModeChange } = props;
  const [activeTab, setActiveTab] = useState<SubTab>("requisitions");
  const [articles, setArticles] = useState<InventoryArticle[]>([]);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [suppliers, setSuppliers] = useState<ProcurementSupplier[]>([]);
  const [requisitions, setRequisitions] = useState<ProcurementRequisition[]>([]);
  const [selectedReqId, setSelectedReqId] = useState<number | null>(null);
  const [reqLines, setReqLines] = useState<ProcurementRequisitionLine[]>([]);
  const [purchaseOrders, setPurchaseOrders] = useState<PurchaseOrder[]>([]);
  const [selectedPoId, setSelectedPoId] = useState<number | null>(null);
  const [poLines, setPoLines] = useState<PurchaseOrderLine[]>([]);
  const [repairables, setRepairables] = useState<RepairableOrder[]>([]);
  const [stateEvents, setStateEvents] = useState<InventoryStateEvent[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [reqArticleId, setReqArticleId] = useState<number>(0);
  const [reqLocationId, setReqLocationId] = useState<number>(0);
  const [reqQty, setReqQty] = useState<number>(0);
  const [reqSourceType, setReqSourceType] = useState("REORDER");
  const [reqSourceId, setReqSourceId] = useState<string>("");
  const [reqSourceRef, setReqSourceRef] = useState<string>("");
  const [reqReservationId, setReqReservationId] = useState<string>("");
  const [reqReason, setReqReason] = useState<string>("");

  const [poSupplierId, setPoSupplierId] = useState<number>(0);
  const [receiptPoLineId, setReceiptPoLineId] = useState<number>(0);
  const [receiptLocationId, setReceiptLocationId] = useState<number>(0);
  const [receiptQty, setReceiptQty] = useState<number>(0);
  const [receiptRejectQty, setReceiptRejectQty] = useState<number>(0);

  const [repairArticleId, setRepairArticleId] = useState<number>(0);
  const [repairQty, setRepairQty] = useState<number>(0);
  const [repairSourceLocationId, setRepairSourceLocationId] = useState<number>(0);
  const [repairReturnLocationId, setRepairReturnLocationId] = useState<number>(0);
  const [repairReason, setRepairReason] = useState<string>("");
  const [repairBalances, setRepairBalances] = useState<InventoryStockBalance[]>([]);
  const [internalReqView, setInternalReqView] = useState<"list" | "kanban">("list");
  const reqView = viewModeProp ?? internalReqView;
  const setReqView = onViewModeChange ?? setInternalReqView;
  const viewFromParent = onViewModeChange !== undefined;
  const [receiveOpen, setReceiveOpen] = useState(false);
  const [createReqOpen, setCreateReqOpen] = useState(false);
  const [createPoOpen, setCreatePoOpen] = useState(false);
  const [repairOpen, setRepairOpen] = useState(false);
  const [reqDetailOpen, setReqDetailOpen] = useState(false);
  const [poDetailOpen, setPoDetailOpen] = useState(false);

  const [poDialogReqId, setPoDialogReqId] = useState<number>(0);
  const [reqStatusFilter, setReqStatusFilter] = useState<string>("__all__");
  const [poStatusFilter, setPoStatusFilter] = useState<string>("__all__");
  const [repairStatusFilter, setRepairStatusFilter] = useState<string>("__all__");
  const [eventArticleFilter, setEventArticleFilter] = useState<string>("__all__");
  const [eventWarehouseFilter, setEventWarehouseFilter] = useState<string>("__all__");
  const [eventFromDate, setEventFromDate] = useState<string>("");
  const [eventToDate, setEventToDate] = useState<string>("");
  const REQ_KANBAN_STATUSES = ["DRAFT", "SUBMITTED", "APPROVED", "CLOSED"] as const;
  const PO_KANBAN_STATUSES = ["DRAFT", "SUBMITTED", "APPROVED", "CLOSED"] as const;

  useImperativeHandle(ref, () => ({
    openCreateRequisition: () => {
      setActiveTab("requisitions");
      setCreateReqOpen(true);
    },
    openCreatePo: () => {
      setActiveTab("purchase-orders");
      const approved = requisitions.find((r) => r.status === "APPROVED");
      setPoDialogReqId(approved?.id ?? 0);
      setCreatePoOpen(true);
    },
    openReceiveGoods: () => {
      setActiveTab("purchase-orders");
      setReceiveOpen(true);
    },
    openRepairable: () => {
      setActiveTab("repairables");
      setRepairOpen(true);
    },
  }));

  const loadAll = async () => {
    setLoading(true);
    setError(null);
    try {
      const [articleRows, locationRows, supplierRows, requisitionRows, poRows, repairRows, eventRows] = await Promise.all([
        listInventoryArticles({ search: null }),
        listInventoryLocations(null),
        listInventoryProcurementSuppliers(),
        listInventoryProcurementRequisitions(),
        listInventoryPurchaseOrders(),
        listInventoryRepairableOrders(),
        listInventoryStateEvents(undefined, undefined),
      ]);
      setArticles(articleRows.filter((a) => a.is_active === 1));
      setLocations(locationRows.filter((l) => l.is_active === 1));
      setSuppliers(supplierRows.filter((s) => s.is_active === 1));
      setRequisitions(requisitionRows);
      setPurchaseOrders(poRows);
      setRepairables(repairRows);
      setStateEvents(eventRows.slice(0, 20));
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadAll();
  }, []);

  useEffect(() => {
    if (repairArticleId <= 0) {
      setRepairBalances([]);
      return;
    }
    void listInventoryStockBalances({
      article_id: repairArticleId,
      warehouse_id: null,
    })
      .then(setRepairBalances)
      .catch(() => setRepairBalances([]));
  }, [repairArticleId]);

  useEffect(() => {
    if (!selectedReqId) {
      setReqLines([]);
      return;
    }
    void listInventoryProcurementRequisitionLines(selectedReqId)
      .then(setReqLines)
      .catch((err) => setError(toErrorMessage(err)));
  }, [selectedReqId]);

  useEffect(() => {
    if (!selectedPoId) {
      setPoLines([]);
      return;
    }
    void listInventoryPurchaseOrderLines(selectedPoId)
      .then(setPoLines)
      .catch((err) => setError(toErrorMessage(err)));
  }, [selectedPoId]);

  const selectedRequisition = useMemo(
    () => requisitions.find((row) => row.id === selectedReqId) ?? null,
    [requisitions, selectedReqId],
  );
  const selectedPo = useMemo(
    () => purchaseOrders.find((row) => row.id === selectedPoId) ?? null,
    [purchaseOrders, selectedPoId],
  );

  const runSaving = async (work: () => Promise<void>) => {
    setSaving(true);
    setError(null);
    try {
      await work();
      await loadAll();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const openRequisitionDetails = (reqId: number) => {
    setSelectedReqId(reqId);
    setReqDetailOpen(true);
  };

  const transitionSelectedRequisition = async (nextStatus: string) => {
    if (!selectedRequisition) return;
    await runSaving(async () => {
      await transitionInventoryProcurementRequisition({
        requisition_id: selectedRequisition.id,
        expected_row_version: selectedRequisition.row_version,
        next_status: nextStatus,
      });
    });
  };

  const openPoDetails = (poId: number) => {
    setSelectedPoId(poId);
    setPoDetailOpen(true);
  };

  const transitionSelectedPo = async (nextStatus: string) => {
    if (!selectedPo) return;
    await runSaving(async () => {
      await transitionInventoryPurchaseOrder({
        purchase_order_id: selectedPo.id,
        expected_row_version: selectedPo.row_version,
        next_status: nextStatus,
      });
    });
  };

  const subTabs: Array<{ id: SubTab; label: string }> = [
    { id: "requisitions", label: "Requisitions" },
    { id: "purchase-orders", label: "Purchase orders" },
    { id: "repairables", label: "Repairables" },
    { id: "lifecycle", label: "Last lifecycle events" },
  ];

  const reqColumns: ColumnDef<ProcurementRequisition>[] = useMemo(
    () => [
      { accessorKey: "req_number", header: "Requisition" },
      { accessorKey: "status", header: "Status" },
      { accessorKey: "demand_source_type", header: "Source" },
      { accessorKey: "updated_at", header: "Updated" },
    ],
    [],
  );

  const poColumns: ColumnDef<PurchaseOrder>[] = useMemo(
    () => [
      { accessorKey: "po_number", header: "PO" },
      { accessorKey: "status", header: "Status" },
      { accessorKey: "supplier_company_name", header: "Supplier" },
      { accessorKey: "updated_at", header: "Updated" },
    ],
    [],
  );

  const repairColumns: ColumnDef<RepairableOrder>[] = useMemo(
    () => [
      { accessorKey: "order_code", header: "Order" },
      { accessorKey: "article_code", header: "Article" },
      { accessorKey: "source_location_code", header: "Source" },
      { accessorKey: "status", header: "Status" },
      { accessorKey: "updated_at", header: "Updated" },
    ],
    [],
  );

  const eventColumns: ColumnDef<InventoryStateEvent>[] = useMemo(
    () => [
      { accessorKey: "entity_type", header: "Entity" },
      { accessorKey: "entity_id", header: "ID" },
      {
        id: "transition",
        header: "Transition",
        cell: ({ row }) => `${row.original.from_status ?? "—"} -> ${row.original.to_status}`,
      },
      { accessorKey: "reason", header: "Reason" },
      { accessorKey: "changed_at", header: "Changed" },
    ],
    [],
  );

  const approvedRequisitions = useMemo(
    () => requisitions.filter((r) => r.status === "APPROVED"),
    [requisitions],
  );

  const filteredRequisitions = useMemo(
    () => (reqStatusFilter === "__all__" ? requisitions : requisitions.filter((r) => r.status === reqStatusFilter)),
    [requisitions, reqStatusFilter],
  );

  const filteredPurchaseOrders = useMemo(
    () => (poStatusFilter === "__all__" ? purchaseOrders : purchaseOrders.filter((po) => po.status === poStatusFilter)),
    [purchaseOrders, poStatusFilter],
  );

  const filteredRepairables = useMemo(
    () =>
      repairStatusFilter === "__all__"
        ? repairables
        : repairables.filter((order) => order.status === repairStatusFilter),
    [repairables, repairStatusFilter],
  );

  const warehouseOptions = useMemo(() => {
    const map = new Map<string, string>();
    for (const row of locations) {
      if (!map.has(row.warehouse_code)) map.set(row.warehouse_code, row.warehouse_code);
    }
    return Array.from(map.keys()).sort();
  }, [locations]);

  const repairById = useMemo(() => {
    const map = new Map<number, RepairableOrder>();
    for (const row of repairables) map.set(row.id, row);
    return map;
  }, [repairables]);

  const filteredEvents = useMemo(() => {
    const fromMs = eventFromDate ? new Date(`${eventFromDate}T00:00:00`).getTime() : null;
    const toMs = eventToDate ? new Date(`${eventToDate}T23:59:59`).getTime() : null;

    return stateEvents.filter((event) => {
      const eventMs = new Date(event.changed_at).getTime();
      if (fromMs !== null && eventMs < fromMs) return false;
      if (toMs !== null && eventMs > toMs) return false;

      if (eventArticleFilter !== "__all__") {
        const rep = repairById.get(event.entity_id);
        if (!rep || String(rep.article_id) !== eventArticleFilter) return false;
      }

      if (eventWarehouseFilter !== "__all__") {
        const rep = repairById.get(event.entity_id);
        if (!rep || !rep.source_location_code.startsWith(`${eventWarehouseFilter}/`)) return false;
      }

      return true;
    });
  }, [stateEvents, eventFromDate, eventToDate, eventArticleFilter, eventWarehouseFilter, repairById]);

  const requisitionForm = (
    <div className="grid gap-2 md:grid-cols-2">
      <div className="space-y-1">
        <Label>Article</Label>
        <Select value={String(reqArticleId)} onValueChange={(v) => setReqArticleId(Number(v))}>
          <SelectTrigger>
            <SelectValue placeholder="Article" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Select article</SelectItem>
            {articles.map((a) => (
              <SelectItem key={a.id} value={String(a.id)}>
                {a.article_code} - {a.article_name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label>Preferred location</Label>
        <Select value={String(reqLocationId)} onValueChange={(v) => setReqLocationId(Number(v))}>
          <SelectTrigger>
            <SelectValue placeholder="Preferred location" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">No preferred location</SelectItem>
            {locations.map((l) => (
              <SelectItem key={l.id} value={String(l.id)}>
                {l.warehouse_code}/{l.code}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label htmlFor="inv-req-qty">Requested quantity</Label>
        <Input
          id="inv-req-qty"
          type="number"
          min={0}
          step="0.01"
          value={reqQty}
          onChange={(e) => setReqQty(Number(e.target.value || 0))}
        />
      </div>
      <div className="space-y-1">
        <Label>Demand source type</Label>
        <Select value={reqSourceType} onValueChange={setReqSourceType}>
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="REORDER">REORDER</SelectItem>
            <SelectItem value="RESERVATION">RESERVATION</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label>Demand source id (optional)</Label>
        <Input value={reqSourceId} onChange={(e) => setReqSourceId(e.target.value)} />
      </div>
      <div className="space-y-1">
        <Label>Demand source ref (optional)</Label>
        <Input value={reqSourceRef} onChange={(e) => setReqSourceRef(e.target.value)} />
      </div>
      <div className="space-y-1">
        <Label>Reservation id (optional)</Label>
        <Input value={reqReservationId} onChange={(e) => setReqReservationId(e.target.value)} />
      </div>
      <div className="space-y-1 md:col-span-2">
        <Label>Reason</Label>
        <Input value={reqReason} onChange={(e) => setReqReason(e.target.value)} />
      </div>
    </div>
  );

  const receiveForm = (
    <div className="grid gap-2">
      <div className="space-y-1">
        <Label>Purchase order</Label>
        <Select value={String(selectedPoId ?? 0)} onValueChange={(v) => setSelectedPoId(Number(v) || null)}>
          <SelectTrigger>
            <SelectValue placeholder="Select PO" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Select PO</SelectItem>
            {purchaseOrders.map((po) => (
              <SelectItem key={po.id} value={String(po.id)}>
                {po.po_number} ({po.status})
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label>PO line</Label>
        <Select value={String(receiptPoLineId)} onValueChange={(v) => setReceiptPoLineId(Number(v))}>
          <SelectTrigger>
            <SelectValue placeholder="PO line" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Select line</SelectItem>
            {poLines.map((line) => (
              <SelectItem key={line.id} value={String(line.id)}>
                {line.article_code} ordered {line.ordered_qty}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label>Receipt location</Label>
        <Select value={String(receiptLocationId)} onValueChange={(v) => setReceiptLocationId(Number(v))}>
          <SelectTrigger>
            <SelectValue placeholder="Receipt location" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="0">Select location</SelectItem>
            {locations.map((l) => (
              <SelectItem key={l.id} value={String(l.id)}>
                {l.warehouse_code}/{l.code}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label>Received qty</Label>
        <Input
          type="number"
          min={0}
          step="0.01"
          value={receiptQty}
          onChange={(e) => setReceiptQty(Number(e.target.value || 0))}
        />
      </div>
      <div className="space-y-1">
        <Label>Rejected qty</Label>
        <Input
          type="number"
          min={0}
          step="0.01"
          value={receiptRejectQty}
          onChange={(e) => setReceiptRejectQty(Number(e.target.value || 0))}
        />
      </div>
    </div>
  );

  return (
    <div className="space-y-4">
      {error ? <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2 text-sm">{error}</div> : null}

      <div className="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
        <div className="rounded-md border p-2">
          <div className="space-y-1">
            {subTabs.map((tab) => (
              <button
                key={tab.id}
                type="button"
                onClick={() => setActiveTab(tab.id)}
                className={cn(
                  "w-full rounded-md px-3 py-2 text-left text-sm transition",
                  activeTab === tab.id ? "bg-primary text-primary-foreground" : "hover:bg-muted",
                )}
              >
                {tab.label}
              </button>
            ))}
          </div>
        </div>

        <div className="min-w-0 space-y-4">
      {activeTab === "requisitions" ? (
      <div className="rounded-md border p-4">
        <h3 className="mb-2 text-sm font-semibold">Requisitions</h3>
        <div className="mb-2 flex flex-wrap items-center gap-2">
          <Select value={reqStatusFilter} onValueChange={setReqStatusFilter}>
            <SelectTrigger className="h-8 w-[180px] text-sm"><SelectValue placeholder="Status" /></SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">All statuses</SelectItem>
              {Array.from(new Set(requisitions.map((r) => r.status))).map((status) => (
                <SelectItem key={status} value={status}>{status}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          {!viewFromParent ? (
            <div className="flex items-center gap-0.5 rounded-md border p-0.5">
              <Button
                type="button"
                size="sm"
                variant={reqView === "list" ? "default" : "ghost"}
                className="h-7 px-2"
                onClick={() => setReqView("list")}
                title="List view"
              >
                <List className="h-3.5 w-3.5" />
              </Button>
              <Button
                type="button"
                size="sm"
                variant={reqView === "kanban" ? "default" : "ghost"}
                className="h-7 px-2"
                onClick={() => setReqView("kanban")}
                title="Kanban view"
              >
                <Columns3 className="h-3.5 w-3.5" />
              </Button>
            </div>
          ) : null}
        </div>
        {reqView === "kanban" ? (
          <div className="mb-3 flex gap-3 overflow-x-auto pb-2">
            {REQ_KANBAN_STATUSES.map((status) => {
              const rows = filteredRequisitions.filter((row) => row.status === status);
              return (
                <div key={status} className="min-w-[240px] flex-shrink-0 rounded-md border bg-muted/30 p-2">
                  <div className="mb-2 flex items-center justify-between text-xs font-semibold">
                    <span>{status}</span>
                    <span className="rounded bg-background px-1.5 py-0.5">{rows.length}</span>
                  </div>
                  <div className="space-y-2">
                    {rows.map((req) => (
                      <button
                        key={req.id}
                        type="button"
                        className={cn(
                          "w-full rounded border bg-background p-2 text-left text-xs shadow-sm hover:bg-accent",
                          selectedReqId === req.id ? "ring-2 ring-primary" : "",
                        )}
                        onClick={() => openRequisitionDetails(req.id)}
                      >
                        <div className="font-mono">{req.req_number}</div>
                        <div className="mt-1 text-[11px] text-text-muted">{req.demand_source_type}</div>
                      </button>
                    ))}
                    {rows.length === 0 ? (
                      <div className="rounded border border-dashed bg-background/50 px-2 py-3 text-center text-[11px] text-text-muted">
                        No requests
                      </div>
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        ) : null}
        {reqView === "list" ? (
          <div className="mb-3">
            <DataTable
              columns={reqColumns}
              data={filteredRequisitions}
              isLoading={loading}
              searchable={false}
              onRowClick={(req) => openRequisitionDetails(req.id)}
            />
          </div>
        ) : null}
      </div>
      ) : null}

      {activeTab === "purchase-orders" ? (
      <PermissionGate permission="inv.procure">
        <div className="rounded-md border p-4">
          <h3 className="mb-3 text-sm font-semibold">Purchase orders</h3>
          <div className="mb-3 flex flex-wrap gap-2">
            <Select value={poStatusFilter} onValueChange={setPoStatusFilter}>
              <SelectTrigger className="h-8 w-[180px] text-sm"><SelectValue placeholder="Status" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="__all__">All statuses</SelectItem>
                {Array.from(new Set(purchaseOrders.map((po) => po.status))).map((status) => (
                  <SelectItem key={status} value={status}>{status}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {reqView === "kanban" ? (
            <div className="mb-3 flex gap-3 overflow-x-auto pb-2">
              {PO_KANBAN_STATUSES.map((status) => {
                const rows = filteredPurchaseOrders.filter((row) => row.status === status);
                return (
                  <div key={status} className="min-w-[240px] flex-shrink-0 rounded-md border bg-muted/30 p-2">
                    <div className="mb-2 flex items-center justify-between text-xs font-semibold">
                      <span>{status}</span>
                      <span className="rounded bg-background px-1.5 py-0.5">{rows.length}</span>
                    </div>
                    <div className="space-y-2">
                      {rows.map((po) => (
                        <button
                          key={po.id}
                          type="button"
                          className={cn(
                            "w-full rounded border bg-background p-2 text-left text-xs shadow-sm hover:bg-accent",
                            selectedPoId === po.id ? "ring-2 ring-primary" : "",
                          )}
                          onClick={() => openPoDetails(po.id)}
                        >
                          <div className="font-mono">{po.po_number}</div>
                          <div className="mt-1 text-[11px] text-text-muted">{po.supplier_company_name ?? "No supplier"}</div>
                        </button>
                      ))}
                      {rows.length === 0 ? (
                        <div className="rounded border border-dashed bg-background/50 px-2 py-3 text-center text-[11px] text-text-muted">
                          No purchase orders
                        </div>
                      ) : null}
                    </div>
                  </div>
                );
              })}
            </div>
          ) : (
            <DataTable
              columns={poColumns}
              data={filteredPurchaseOrders}
              isLoading={loading}
              searchable={false}
              onRowClick={(po) => openPoDetails(po.id)}
            />
          )}
        </div>
      </PermissionGate>
      ) : null}

      {activeTab === "repairables" ? (
      <PermissionGate permission="inv.procure">
        <div className="rounded-md border p-4">
          <h3 className="mb-2 text-sm font-semibold">Repairables</h3>
          <div className="mb-3 flex flex-wrap gap-2">
            <Select value={repairStatusFilter} onValueChange={setRepairStatusFilter}>
              <SelectTrigger className="h-8 w-[180px] text-sm"><SelectValue placeholder="Status" /></SelectTrigger>
              <SelectContent>
                <SelectItem value="__all__">All statuses</SelectItem>
                {Array.from(new Set(repairables.map((order) => order.status))).map((status) => (
                  <SelectItem key={status} value={status}>{status}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <DataTable columns={repairColumns} data={filteredRepairables} isLoading={loading} searchable={false} />
          <div className="mt-3 space-y-2 text-xs">
            {filteredRepairables.slice(0, 8).map((order) => (
              <div key={order.id} className="flex flex-wrap items-center gap-2 rounded border p-2">
                <span>{order.order_code}</span>
                <span>{order.status}</span>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || order.status !== "REQUESTED"}
                  onClick={() => void runSaving(async () => {
                    await transitionInventoryRepairableOrder({
                      order_id: order.id,
                      expected_row_version: order.row_version,
                      next_status: "RELEASED",
                    });
                  })}
                >
                  Release
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || order.status !== "RELEASED"}
                  onClick={() => void runSaving(async () => {
                    await transitionInventoryRepairableOrder({
                      order_id: order.id,
                      expected_row_version: order.row_version,
                      next_status: "SENT_FOR_REPAIR",
                    });
                  })}
                >
                  Send
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || order.status !== "SENT_FOR_REPAIR"}
                  onClick={() => void runSaving(async () => {
                    await transitionInventoryRepairableOrder({
                      order_id: order.id,
                      expected_row_version: order.row_version,
                      next_status: "RETURNED_FROM_REPAIR",
                      return_location_id: order.return_location_id,
                    });
                  })}
                >
                  Receive back
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={saving || order.status !== "RETURNED_FROM_REPAIR"}
                  onClick={() => void runSaving(async () => {
                    await transitionInventoryRepairableOrder({
                      order_id: order.id,
                      expected_row_version: order.row_version,
                      next_status: "CLOSED",
                    });
                  })}
                >
                  Close
                </Button>
              </div>
            ))}
          </div>
        </div>
      </PermissionGate>
      ) : null}

      <PermissionGate permission="inv.procure">
        <Dialog open={reqDetailOpen} onOpenChange={setReqDetailOpen}>
          <DialogContent className="max-h-[85vh] max-w-xl overflow-y-auto">
            <DialogHeader>
              <DialogTitle>
                {selectedRequisition ? `${selectedRequisition.req_number} - ${selectedRequisition.status}` : "Request details"}
              </DialogTitle>
            </DialogHeader>
            {selectedRequisition ? (
              <div className="space-y-3">
                <div className="rounded-md border p-3 text-sm">
                  <div><span className="text-text-muted">Source type:</span> {selectedRequisition.demand_source_type}</div>
                  <div><span className="text-text-muted">Source ref:</span> {selectedRequisition.demand_source_ref ?? "—"}</div>
                  <div><span className="text-text-muted">Created:</span> {selectedRequisition.created_at}</div>
                  <div><span className="text-text-muted">Updated:</span> {selectedRequisition.updated_at}</div>
                </div>
                <div className="rounded-md border p-3 text-sm">
                  <div className="mb-2 font-medium">Lines</div>
                  {reqLines.length === 0 ? (
                    <div className="text-text-muted">No lines</div>
                  ) : (
                    <div className="space-y-1">
                      {reqLines.map((line) => (
                        <div key={line.id}>
                          {line.article_code} - {line.article_name} x {line.requested_qty}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <p className="text-sm text-text-muted">No requisition selected.</p>
            )}
            <DialogFooter>
              <Button variant="outline" onClick={() => setReqDetailOpen(false)}>
                Close
              </Button>
              {selectedRequisition ? (
                <>
                  <Button
                    variant="outline"
                    disabled={saving || selectedRequisition.status !== "DRAFT"}
                    onClick={() => void transitionSelectedRequisition("SUBMITTED")}
                  >
                    Submit
                  </Button>
                  <Button
                    disabled={saving || selectedRequisition.status !== "SUBMITTED"}
                    onClick={() => void transitionSelectedRequisition("APPROVED")}
                  >
                    Approve
                  </Button>
                  <Button
                    variant="destructive"
                    disabled={saving || !["DRAFT", "SUBMITTED", "APPROVED"].includes(selectedRequisition.status)}
                    onClick={() => void transitionSelectedRequisition("CLOSED")}
                  >
                    Decline
                  </Button>
                </>
              ) : null}
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={poDetailOpen} onOpenChange={setPoDetailOpen}>
          <DialogContent className="max-h-[85vh] max-w-xl overflow-y-auto">
            <DialogHeader>
              <DialogTitle>
                {selectedPo ? `${selectedPo.po_number} - ${selectedPo.status}` : "Purchase order details"}
              </DialogTitle>
            </DialogHeader>
            {selectedPo ? (
              <div className="space-y-3">
                <div className="rounded-md border p-3 text-sm">
                  <div><span className="text-text-muted">Supplier:</span> {selectedPo.supplier_company_name ?? "No supplier"}</div>
                  <div><span className="text-text-muted">Posting:</span> {selectedPo.posting_state}</div>
                  <div><span className="text-text-muted">Created:</span> {selectedPo.created_at}</div>
                  <div><span className="text-text-muted">Updated:</span> {selectedPo.updated_at}</div>
                </div>
                <div className="rounded-md border p-3 text-sm">
                  <div className="mb-2 font-medium">PO lines</div>
                  {poLines.length === 0 ? (
                    <div className="text-text-muted">No lines</div>
                  ) : (
                    <div className="space-y-1">
                      {poLines.map((line) => (
                        <div key={line.id}>
                          {line.article_code} - {line.article_name} x {line.ordered_qty}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            ) : (
              <p className="text-sm text-text-muted">No purchase order selected.</p>
            )}
            <DialogFooter>
              <Button variant="outline" onClick={() => setPoDetailOpen(false)}>
                Close
              </Button>
              {selectedPo ? (
                <>
                  <Button
                    variant="outline"
                    disabled={saving || selectedPo.status !== "DRAFT"}
                    onClick={() => void transitionSelectedPo("SUBMITTED")}
                  >
                    Submit
                  </Button>
                  <Button
                    disabled={saving || selectedPo.status !== "SUBMITTED"}
                    onClick={() => void transitionSelectedPo("APPROVED")}
                  >
                    Approve
                  </Button>
                  <Button
                    variant="destructive"
                    disabled={saving || !["DRAFT", "SUBMITTED", "APPROVED"].includes(selectedPo.status)}
                    onClick={() => void transitionSelectedPo("CLOSED")}
                  >
                    Decline
                  </Button>
                </>
              ) : null}
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={createReqOpen} onOpenChange={setCreateReqOpen}>
          <DialogContent
            className="max-h-[90vh] max-w-2xl overflow-y-auto"
            onPointerDownOutside={(e) => e.preventDefault()}
          >
            <DialogHeader>
              <DialogTitle>New requisition</DialogTitle>
            </DialogHeader>
            {requisitionForm}
            <DialogFooter className="gap-2 sm:gap-0">
              <Button type="button" variant="outline" onClick={() => setCreateReqOpen(false)}>
                Cancel
              </Button>
              <Button
                type="button"
                disabled={saving || reqArticleId <= 0 || reqQty <= 0}
                onClick={() =>
                  void runSaving(async () => {
                    await createInventoryProcurementRequisition({
                      article_id: reqArticleId,
                      preferred_location_id: reqLocationId > 0 ? reqLocationId : null,
                      requested_qty: reqQty,
                      demand_source_type: reqSourceType,
                      demand_source_id: reqSourceId.trim() ? Number(reqSourceId) : null,
                      demand_source_ref: reqSourceRef.trim() || null,
                      source_reservation_id: reqReservationId.trim() ? Number(reqReservationId) : null,
                      source_reorder_trigger: reqSourceType === "REORDER" ? "threshold_crossed" : null,
                      reason: reqReason.trim() || null,
                      actor_id: null,
                    });
                    setCreateReqOpen(false);
                  })
                }
              >
                Create requisition
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={createPoOpen} onOpenChange={setCreatePoOpen}>
          <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>Create purchase order</DialogTitle>
            </DialogHeader>
            <div className="grid gap-3 py-2">
              <div className="space-y-1">
                <Label>Approved requisition</Label>
                <Select value={String(poDialogReqId)} onValueChange={(v) => setPoDialogReqId(Number(v))}>
                  <SelectTrigger>
                    <SelectValue placeholder="Select requisition" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">Select requisition</SelectItem>
                    {approvedRequisitions.map((req) => (
                      <SelectItem key={req.id} value={String(req.id)}>
                        {req.req_number}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>Supplier</Label>
                <Select value={String(poSupplierId)} onValueChange={(v) => setPoSupplierId(Number(v))}>
                  <SelectTrigger>
                    <SelectValue placeholder="Supplier" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">No supplier</SelectItem>
                    {suppliers.map((s) => (
                      <SelectItem key={s.id} value={String(s.id)}>
                        {s.company_code} - {s.company_name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
            <DialogFooter className="gap-2 sm:gap-0">
              <Button type="button" variant="outline" onClick={() => setCreatePoOpen(false)}>
                Cancel
              </Button>
              <Button
                type="button"
                disabled={
                  saving ||
                  poDialogReqId <= 0 ||
                  !approvedRequisitions.some((r) => r.id === poDialogReqId)
                }
                onClick={() =>
                  void runSaving(async () => {
                    await createInventoryPurchaseOrderFromRequisition({
                      requisition_id: poDialogReqId,
                      supplier_company_id: poSupplierId > 0 ? poSupplierId : null,
                      actor_id: null,
                    });
                    setCreatePoOpen(false);
                  })
                }
              >
                Create PO
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={receiveOpen} onOpenChange={setReceiveOpen}>
          <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>Post goods receipt</DialogTitle>
            </DialogHeader>
            <p className="text-xs text-muted-foreground">
              Select an approved PO and line, then post quantities to a stock location.
            </p>
            {receiveForm}
            <DialogFooter className="gap-2 sm:gap-0">
              <Button type="button" variant="outline" onClick={() => setReceiveOpen(false)}>
                Cancel
              </Button>
              <Button
                type="button"
                disabled={saving || !selectedPo || receiptPoLineId <= 0 || receiptLocationId <= 0 || receiptQty <= 0}
                onClick={() =>
                  void runSaving(async () => {
                    const line = poLines.find((item) => item.id === receiptPoLineId);
                    if (!line || !selectedPo) return;
                    await receiveInventoryPurchaseOrderGoods({
                      purchase_order_id: selectedPo.id,
                      lines: [
                        {
                          po_line_id: line.id,
                          article_id: line.article_id,
                          location_id: receiptLocationId,
                          received_qty: receiptQty,
                          accepted_qty: Math.max(0, receiptQty - receiptRejectQty),
                          rejected_qty: receiptRejectQty,
                        },
                      ],
                    });
                    setReceiveOpen(false);
                  })
                }
              >
                Post GR
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog open={repairOpen} onOpenChange={setRepairOpen}>
          <DialogContent
            className="max-h-[90vh] max-w-2xl overflow-y-auto"
            onPointerDownOutside={(e) => e.preventDefault()}
          >
            <DialogHeader>
              <DialogTitle>New repairable order</DialogTitle>
            </DialogHeader>
            <div className="grid gap-2 md:grid-cols-2">
              <div className="space-y-1">
                <Label>Article</Label>
                <Select value={String(repairArticleId)} onValueChange={(v) => setRepairArticleId(Number(v))}>
                  <SelectTrigger>
                    <SelectValue placeholder="Article" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">Select article</SelectItem>
                    {articles.map((a) => (
                      <SelectItem key={a.id} value={String(a.id)}>
                        {a.article_code}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>Source location</Label>
                <Select value={String(repairSourceLocationId)} onValueChange={(v) => setRepairSourceLocationId(Number(v))}>
                  <SelectTrigger>
                    <SelectValue placeholder="Source location" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">Select source</SelectItem>
                    {locations.map((l) => (
                      <SelectItem key={l.id} value={String(l.id)}>
                        {l.warehouse_code}/{l.code}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>Return location</Label>
                <Select value={String(repairReturnLocationId)} onValueChange={(v) => setRepairReturnLocationId(Number(v))}>
                  <SelectTrigger>
                    <SelectValue placeholder="Return location" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">No return location</SelectItem>
                    {locations.map((l) => (
                      <SelectItem key={l.id} value={String(l.id)}>
                        {l.warehouse_code}/{l.code}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>Quantity</Label>
                <Input
                  type="number"
                  min={0}
                  step="0.01"
                  value={repairQty}
                  onChange={(e) => setRepairQty(Number(e.target.value || 0))}
                />
              </div>
              <div className="space-y-1 md:col-span-2">
                <Label>Reason</Label>
                <Input value={repairReason} onChange={(e) => setRepairReason(e.target.value)} />
              </div>
            </div>
            {repairArticleId > 0 ? (
              <div className="mt-3 rounded-md border border-dashed p-2 text-xs">
                <div className="mb-1 font-semibold">Stock balances for selected article</div>
                {repairBalances.length === 0 ? (
                  <div className="text-muted-foreground">No balance rows (or zero stock) for this article.</div>
                ) : (
                  <ul className="space-y-1">
                    {repairBalances.map((b) => (
                      <li key={`${b.id}-${b.location_id}`}>
                        {b.warehouse_code}/{b.location_code}: on-hand {b.on_hand_qty}, reserved {b.reserved_qty}, available{" "}
                        {b.available_qty}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            ) : null}
            <DialogFooter className="gap-2 sm:gap-0">
              <Button type="button" variant="outline" onClick={() => setRepairOpen(false)}>
                Cancel
              </Button>
              <Button
                type="button"
                disabled={saving || repairArticleId <= 0 || repairSourceLocationId <= 0 || repairQty <= 0}
                onClick={() =>
                  void runSaving(async () => {
                    await createInventoryRepairableOrder({
                      article_id: repairArticleId,
                      quantity: repairQty,
                      source_location_id: repairSourceLocationId,
                      return_location_id: repairReturnLocationId > 0 ? repairReturnLocationId : null,
                      reason: repairReason.trim() || null,
                    });
                    setRepairOpen(false);
                  })
                }
              >
                Create repairable order
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </PermissionGate>

      {activeTab === "lifecycle" ? (
      <div className="rounded-md border p-4">
        <h3 className="mb-2 text-sm font-semibold">Latest lifecycle events</h3>
        <div className="mb-3 grid gap-2 md:grid-cols-4">
          <Select value={eventArticleFilter} onValueChange={setEventArticleFilter}>
            <SelectTrigger className="h-8 text-sm"><SelectValue placeholder="Article" /></SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">All articles</SelectItem>
              {articles.map((a) => (
                <SelectItem key={a.id} value={String(a.id)}>{a.article_code}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Select value={eventWarehouseFilter} onValueChange={setEventWarehouseFilter}>
            <SelectTrigger className="h-8 text-sm"><SelectValue placeholder="Warehouse" /></SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">All warehouses</SelectItem>
              {warehouseOptions.map((w) => (
                <SelectItem key={w} value={w}>{w}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Input type="date" value={eventFromDate} onChange={(e) => setEventFromDate(e.target.value)} />
          <Input type="date" value={eventToDate} onChange={(e) => setEventToDate(e.target.value)} />
        </div>
        <DataTable columns={eventColumns} data={filteredEvents} isLoading={loading} searchable={false} />
      </div>
      ) : null}
        </div>
      </div>
    </div>
  );
});
