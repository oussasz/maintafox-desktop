import type { ColumnDef } from "@tanstack/react-table";
import { Columns3, List } from "lucide-react";
import { forwardRef, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import { PurchaseOrderWorkspace } from "@/components/inventory/PurchaseOrderWorkspace";
import { ProcurementAlertCards } from "@/components/inventory/ProcurementAlertCards";
import { ProcurementRecommendationsPanel } from "@/components/inventory/ProcurementRecommendationsPanel";
import { RepairableDetailDialog } from "@/components/inventory/RepairableDetailDialog";
import { SupplierArticleSourcesSection } from "@/components/inventory/SupplierArticleSourcesSection";
import { SupplierContactsSection } from "@/components/inventory/SupplierContactsSection";
import { riskBadgeVariant } from "@/components/inventory/supplier-sourcing";
import { Badge } from "@/components/ui/badge";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { formatAssetLabel, formatEntityCode } from "@/lib/display";
import { cn } from "@/lib/utils";
import {
  createInventoryProcurementRequisition,
  createInventoryPurchaseOrderFromRequisition,
  createInventoryRepairableOrder,
  deactivateInventorySupplier,
  getInventorySupplierScorecard,
  getProcurementAlerts,
  getProcurementDashboardSummary,
  listInventoryArticles,
  listInventoryDocumentLinks,
  listInventoryLocations,
  listInventoryProcurementRequisitionLines,
  listInventoryProcurementRequisitions,
  listInventoryProcurementSuppliers,
  listInventoryPurchaseOrderLines,
  listInventoryPurchaseOrders,
  listInventoryRepairableOrders,
  listInventoryStateEvents,
  listInventoryStockBalances,
  listInventorySuppliers,
  listSupplierPrices,
  listSupplierPurchaseHistory,
  receiveInventoryPurchaseOrderGoods,
  transitionInventoryProcurementRequisition,
  transitionInventoryRepairableOrder,
  upsertInventoryDocumentLink,
  upsertInventorySupplier,
  upsertSupplierPrice,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  InventoryArticle,
  InventoryDocumentLink,
  InventoryStateEvent,
  InventoryStockBalance,
  InventorySupplier,
  ProcurementAlert,
  ProcurementDashboardSummary,
  ProcurementRequisition,
  ProcurementRequisitionLine,
  ProcurementSupplier,
  PurchaseOrder,
  PurchaseOrderLine,
  RepairableOrder,
  StockLocation,
  SupplierPrice,
  SupplierPurchaseHistoryRow,
  SupplierScorecard,
} from "@shared/ipc-types";

export type ProcurementRepairablePanelHandle = {
  openCreateRequisition: () => void;
  openCreatePo: () => void;
  openReceiveGoods: () => void;
  openRepairable: () => void;
  reload: () => Promise<void>;
};

/** Requisition early-exit target matching backend FSM in procurement.rs */
export function requisitionDeclineOrCloseTarget(status: string): "CANCELLED" | "CLOSED" | null {
  if (status === "DRAFT" || status === "SUBMITTED") return "CANCELLED";
  if (status === "APPROVED" || status === "PARTIALLY_RECEIVED") return "CLOSED";
  return null;
}

/** PO cancel target — never CLOSED (terminal receive is RECEIVED_CLOSED). */
export function purchaseOrderCancelTarget(status: string): "CANCELLED" | null {
  if (["DRAFT", "SUBMITTED", "APPROVED", "PARTIALLY_RECEIVED"].includes(status)) return "CANCELLED";
  return null;
}

type ProcurementRepairablePanelProps = {
  /** When set, list/kanban is controlled by the parent (e.g. Inventory page header). */
  viewMode?: "list" | "kanban";
  onViewModeChange?: (mode: "list" | "kanban") => void;
};

type SubTab = "requisitions" | "purchase-orders" | "repairables" | "suppliers" | "lifecycle";

const SUPPLIER_STATUSES = ["PREFERRED", "APPROVED", "BLOCKED", "UNDER_EVALUATION"] as const;

const PURCHASE_PRIORITIES = ["EMERGENCY", "HIGH", "NORMAL", "LOW"] as const;

const FULFILLMENT_ACTIONS = ["STOCK_ONLY", "RESERVE_FOR_DEMAND", "ISSUE_TO_DEMAND"] as const;

function pctLabel(v: number | null): string {
  if (v == null) return "—";
  return `${v.toFixed(1)}%`;
}

function numLabel(v: number | null, decimals = 1): string {
  if (v == null) return "—";
  return v.toFixed(decimals);
}

function statusBadgeVariant(
  status: string,
): "secondary" | "outline" | "destructive" | "default" {
  if (status === "PREFERRED") return "default";
  if (status === "BLOCKED") return "destructive";
  if (status === "APPROVED") return "secondary";
  return "outline";
}

export const ProcurementRepairablePanel = forwardRef<
  ProcurementRepairablePanelHandle,
  ProcurementRepairablePanelProps
>(function ProcurementRepairablePanel(props, ref) {
  const { viewMode: viewModeProp, onViewModeChange } = props;
  const { t } = useTranslation("inventory");

  // ── Core state ─────────────────────────────────────────────────────────────
  const [activeTab, setActiveTab] = useState<SubTab>("requisitions");
  const [articles, setArticles] = useState<InventoryArticle[]>([]);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [suppliers, setSuppliers] = useState<ProcurementSupplier[]>([]); // legacy list for PO creation
  const [inventorySuppliers, setInventorySuppliers] = useState<InventorySupplier[]>([]);
  const [requisitions, setRequisitions] = useState<ProcurementRequisition[]>([]);
  const [selectedReqId, setSelectedReqId] = useState<number | null>(null);
  const [reqLines, setReqLines] = useState<ProcurementRequisitionLine[]>([]);
  const [purchaseOrders, setPurchaseOrders] = useState<PurchaseOrder[]>([]);
  const [selectedPoId, setSelectedPoId] = useState<number | null>(null);
  const [poLines, setPoLines] = useState<PurchaseOrderLine[]>([]);
  const [repairables, setRepairables] = useState<RepairableOrder[]>([]);
  const [stateEvents, setStateEvents] = useState<InventoryStateEvent[]>([]);
  const [dashboard, setDashboard] = useState<ProcurementDashboardSummary | null>(null);
  const [alerts, setAlerts] = useState<ProcurementAlert[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // ── Requisition form ────────────────────────────────────────────────────────
  const [reqArticleId, setReqArticleId] = useState<number>(0);
  const [reqLocationId, setReqLocationId] = useState<number>(0);
  const [reqQty, setReqQty] = useState<number>(0);
  const [reqSourceType, setReqSourceType] = useState("REORDER");
  const [reqSourceId, setReqSourceId] = useState<string>("");
  const [reqSourceRef, setReqSourceRef] = useState<string>("");
  const [reqReservationId, setReqReservationId] = useState<string>("");
  const [reqReason, setReqReason] = useState<string>("");
  const [reqPriority, setReqPriority] = useState<string>("NORMAL");

  // ── PO form ─────────────────────────────────────────────────────────────────
  const [poSupplierId, setPoSupplierId] = useState<number>(0);
  const [poDialogReqId, setPoDialogReqId] = useState<number>(0);

  // ── Receive form ────────────────────────────────────────────────────────────
  const [receiptPoLineId, setReceiptPoLineId] = useState<number>(0);
  const [receiptLocationId, setReceiptLocationId] = useState<number>(0);
  const [receiptQty, setReceiptQty] = useState<number>(0);
  const [receiptRejectQty, setReceiptRejectQty] = useState<number>(0);
  const [receiptFulfillmentAction, setReceiptFulfillmentAction] = useState<string>("STOCK_ONLY");

  // ── Repairable form ─────────────────────────────────────────────────────────
  const [repairArticleId, setRepairArticleId] = useState<number>(0);
  const [repairQty, setRepairQty] = useState<number>(0);
  const [repairSourceLocationId, setRepairSourceLocationId] = useState<number>(0);
  const [repairReturnLocationId, setRepairReturnLocationId] = useState<number>(0);
  const [repairReason, setRepairReason] = useState<string>("");
  const [repairBalances, setRepairBalances] = useState<InventoryStockBalance[]>([]);

  // ── View modes ──────────────────────────────────────────────────────────────
  const [internalReqView, setInternalReqView] = useState<"list" | "kanban">("list");
  const reqView = viewModeProp ?? internalReqView;
  const setReqView = onViewModeChange ?? setInternalReqView;
  const viewFromParent = onViewModeChange !== undefined;

  // ── Dialog open states ──────────────────────────────────────────────────────
  const [receiveOpen, setReceiveOpen] = useState(false);
  const [createReqOpen, setCreateReqOpen] = useState(false);
  const [createPoOpen, setCreatePoOpen] = useState(false);
  const [repairOpen, setRepairOpen] = useState(false);
  const [reqDetailOpen, setReqDetailOpen] = useState(false);
  const [poDetailOpen, setPoDetailOpen] = useState(false);
  const [repairDetailOpen, setRepairDetailOpen] = useState(false);
  const [selectedRepairableId, setSelectedRepairableId] = useState<number | null>(null);

  // ── Filter states ───────────────────────────────────────────────────────────
  const [reqStatusFilter, setReqStatusFilter] = useState<string>("__all__");
  const [poStatusFilter, setPoStatusFilter] = useState<string>("__all__");
  const [repairStatusFilter, setRepairStatusFilter] = useState<string>("__all__");
  const [eventArticleFilter, setEventArticleFilter] = useState<string>("__all__");
  const [eventWarehouseFilter, setEventWarehouseFilter] = useState<string>("__all__");
  const [eventFromDate, setEventFromDate] = useState<string>("");
  const [eventToDate, setEventToDate] = useState<string>("");

  // ── Supplier directory state ────────────────────────────────────────────────
  const [supplierSearchInput, setSupplierSearchInput] = useState("");
  const [supplierSearch, setSupplierSearch] = useState("");
  const [supplierStatusFilter, setSupplierStatusFilter] = useState("__all__");
  const [selectedSupplierId, setSelectedSupplierId] = useState<number | null>(null);
  const [supplierScorecard, setSupplierScorecard] = useState<SupplierScorecard | null>(null);
  const [supplierPrices, setSupplierPrices] = useState<SupplierPrice[]>([]);
  const [supplierDocs, setSupplierDocs] = useState<InventoryDocumentLink[]>([]);
  const [supplierPurchaseHistory, setSupplierPurchaseHistory] = useState<
    SupplierPurchaseHistoryRow[]
  >([]);
  const [supplierScorecardLoading, setSupplierScorecardLoading] = useState(false);
  const [supplierEditOpen, setSupplierEditOpen] = useState(false);
  const [supplierForm, setSupplierForm] = useState<{
    id: number | null;
    rowVersion: number | null;
    code: string;
    name: string;
    statusCode: string;
    paymentTermsCode: string;
    defaultLeadTimeDays: string;
    isActive: boolean;
  }>({
    id: null,
    rowVersion: null,
    code: "",
    name: "",
    statusCode: "APPROVED",
    paymentTermsCode: "",
    defaultLeadTimeDays: "",
    isActive: true,
  });

  // ── Supplier price form ─────────────────────────────────────────────────────
  const [priceFormOpen, setPriceFormOpen] = useState(false);
  const [priceFormArticleId, setPriceFormArticleId] = useState<number>(0);
  const [priceFormUnitPrice, setPriceFormUnitPrice] = useState<string>("");
  const [priceFormMinQty, setPriceFormMinQty] = useState<string>("");
  const [priceFormValidFrom, setPriceFormValidFrom] = useState<string>("");

  // ── Supplier doc link form ──────────────────────────────────────────────────
  const [docLinkFormOpen, setDocLinkFormOpen] = useState(false);
  const [docLinkFormRef, setDocLinkFormRef] = useState<string>("");
  const [docLinkFormPurpose, setDocLinkFormPurpose] = useState<string>("CONTRACT");
  const [docLinkFormPrimary, setDocLinkFormPrimary] = useState<boolean>(false);

  const REQ_KANBAN_STATUSES = [
    "DRAFT",
    "SUBMITTED",
    "APPROVED",
    "PARTIALLY_RECEIVED",
    "CLOSED",
    "CANCELLED",
  ] as const;

  const PO_KANBAN_STATUSES = [
    "DRAFT",
    "SUBMITTED",
    "APPROVED",
    "PARTIALLY_RECEIVED",
    "RECEIVED_CLOSED",
    "CANCELLED",
  ] as const;

  // ── Data loading ────────────────────────────────────────────────────────────
  const loadAll = async () => {
    setLoading(true);
    setError(null);
    try {
      const [articleRows, locationRows, supplierRows, inventorySupplierRows, requisitionRows, poRows, repairRows, eventRows, dashboardData, alertRows] =
        await Promise.all([
          listInventoryArticles({ search: null }),
          listInventoryLocations(null),
          listInventoryProcurementSuppliers(),
          listInventorySuppliers(),
          listInventoryProcurementRequisitions(),
          listInventoryPurchaseOrders(),
          listInventoryRepairableOrders(),
          listInventoryStateEvents(undefined, undefined),
          getProcurementDashboardSummary(),
          getProcurementAlerts(),
        ]);
      setArticles(articleRows.filter((a) => a.is_active === 1));
      setLocations(locationRows.filter((l) => l.is_active === 1));
      setSuppliers(supplierRows.filter((s) => s.is_active === 1));
      setInventorySuppliers(inventorySupplierRows);
      setRequisitions(requisitionRows);
      setPurchaseOrders(poRows);
      setRepairables(repairRows);
      setStateEvents(eventRows.slice(0, 20));
      setDashboard(dashboardData);
      setAlerts(alertRows);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

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
    reload: () => loadAll(),
  }));

  useEffect(() => {
    void loadAll();
  }, []);

  useEffect(() => {
    if (repairArticleId <= 0) {
      setRepairBalances([]);
      return;
    }
    void listInventoryStockBalances({ article_id: repairArticleId, warehouse_id: null })
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

  useEffect(() => {
    if (!selectedSupplierId) {
      setSupplierScorecard(null);
      setSupplierPrices([]);
      setSupplierDocs([]);
      setSupplierPurchaseHistory([]);
      return;
    }
    setSupplierScorecardLoading(true);
    Promise.all([
      getInventorySupplierScorecard(selectedSupplierId),
      listSupplierPrices(selectedSupplierId),
      listInventoryDocumentLinks("SUPPLIER", selectedSupplierId),
      listSupplierPurchaseHistory(selectedSupplierId),
    ])
      .then(([scorecard, prices, docs, history]) => {
        setSupplierScorecard(scorecard);
        setSupplierPrices(prices);
        setSupplierDocs(docs);
        setSupplierPurchaseHistory(history);
      })
      .catch((err) => setError(toErrorMessage(err)))
      .finally(() => setSupplierScorecardLoading(false));
  }, [selectedSupplierId]);

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

  const openRepairableDetails = (orderId: number) => {
    setSelectedRepairableId(orderId);
    setRepairDetailOpen(true);
  };

  const openNewSupplierForm = () => {
    setSupplierForm({
      id: null,
      rowVersion: null,
      code: "",
      name: "",
      statusCode: "APPROVED",
      paymentTermsCode: "",
      defaultLeadTimeDays: "",
      isActive: true,
    });
    setSupplierEditOpen(true);
  };

  const openEditSupplierForm = (s: InventorySupplier) => {
    setSupplierForm({
      id: s.id,
      rowVersion: s.row_version,
      code: s.code,
      name: s.name,
      statusCode: s.status_code,
      paymentTermsCode: s.payment_terms_code ?? "",
      defaultLeadTimeDays: s.default_lead_time_days != null ? String(s.default_lead_time_days) : "",
      isActive: s.is_active === 1,
    });
    setSupplierEditOpen(true);
  };

  const saveSupplierForm = async () => {
    await runSaving(async () => {
      await upsertInventorySupplier(
        supplierForm.id ?? undefined,
        supplierForm.rowVersion ?? undefined,
        {
          code: supplierForm.code.trim(),
          name: supplierForm.name.trim(),
          status_code: supplierForm.statusCode,
          payment_terms_code: supplierForm.paymentTermsCode.trim() || null,
          default_lead_time_days: supplierForm.defaultLeadTimeDays
            ? Number(supplierForm.defaultLeadTimeDays)
            : null,
          is_active: supplierForm.isActive,
        },
      );
      setSupplierEditOpen(false);
    });
  };

  const savePriceForm = async () => {
    if (!selectedSupplierId || priceFormArticleId <= 0) return;
    await runSaving(async () => {
      await upsertSupplierPrice(undefined, {
        supplier_id: selectedSupplierId,
        article_id: priceFormArticleId,
        unit_price: Number(priceFormUnitPrice),
        min_order_qty: priceFormMinQty ? Number(priceFormMinQty) : null,
        valid_from: priceFormValidFrom || null,
      });
      // Refresh prices
      const prices = await listSupplierPrices(selectedSupplierId);
      setSupplierPrices(prices);
      setPriceFormOpen(false);
    });
  };

  const saveDocLinkForm = async () => {
    if (!selectedSupplierId || !docLinkFormRef.trim()) return;
    await runSaving(async () => {
      await upsertInventoryDocumentLink({
        entity_type: "SUPPLIER",
        entity_id: selectedSupplierId,
        document_ref: docLinkFormRef.trim(),
        link_purpose: docLinkFormPurpose,
        is_primary: docLinkFormPrimary,
      });
      const docs = await listInventoryDocumentLinks("SUPPLIER", selectedSupplierId);
      setSupplierDocs(docs);
      setDocLinkFormOpen(false);
    });
  };

  // ── Tabs ────────────────────────────────────────────────────────────────────
  const subTabs: Array<{ id: SubTab; label: string }> = [
    { id: "requisitions", label: "Requisitions" },
    { id: "purchase-orders", label: "Purchase orders" },
    { id: "repairables", label: "Repairables" },
    { id: "suppliers", label: t("procurement.suppliers.title") },
    { id: "lifecycle", label: "Last lifecycle events" },
  ];

  // ── Column definitions ──────────────────────────────────────────────────────
  const reqColumns: ColumnDef<ProcurementRequisition>[] = useMemo(
    () => [
      { accessorKey: "req_number", header: "Requisition" },
      {
        accessorKey: "purchase_priority",
        header: t("procurement.requisition.priority"),
        cell: ({ row }) => row.original.purchase_priority ?? "—",
      },
      { accessorKey: "status", header: "Status" },
      { accessorKey: "demand_source_type", header: "Source" },
      { accessorKey: "updated_at", header: "Updated" },
    ],
    [t],
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

  const supplierColumns: ColumnDef<InventorySupplier>[] = useMemo(
    () => [
      { accessorKey: "code", header: t("procurement.suppliers.columns.code") },
      { accessorKey: "name", header: t("procurement.suppliers.columns.name") },
      {
        accessorKey: "status_code",
        header: t("procurement.suppliers.columns.status"),
        cell: ({ row }) => (
          <Badge variant={statusBadgeVariant(row.original.status_code)}>
            {t(`procurement.suppliers.status.${row.original.status_code}`, {
              defaultValue: row.original.status_code,
            })}
          </Badge>
        ),
      },
      {
        accessorKey: "default_lead_time_days",
        header: t("procurement.suppliers.columns.leadTime"),
        cell: ({ row }) => row.original.default_lead_time_days ?? "—",
      },
    ],
    [t],
  );

  // ── Derived lists ───────────────────────────────────────────────────────────
  const approvedRequisitions = useMemo(
    () => requisitions.filter((r) => r.status === "APPROVED"),
    [requisitions],
  );

  const filteredRequisitions = useMemo(
    () =>
      reqStatusFilter === "__all__"
        ? requisitions
        : requisitions.filter((r) => r.status === reqStatusFilter),
    [requisitions, reqStatusFilter],
  );

  const filteredPurchaseOrders = useMemo(
    () =>
      poStatusFilter === "__all__"
        ? purchaseOrders
        : purchaseOrders.filter((po) => po.status === poStatusFilter),
    [purchaseOrders, poStatusFilter],
  );

  const filteredRepairables = useMemo(
    () =>
      repairStatusFilter === "__all__"
        ? repairables
        : repairables.filter((order) => order.status === repairStatusFilter),
    [repairables, repairStatusFilter],
  );

  const filteredInventorySuppliers = useMemo(() => {
    const term = supplierSearch.trim().toLowerCase();
    return inventorySuppliers.filter((s) => {
      if (supplierStatusFilter !== "__all__" && s.status_code !== supplierStatusFilter) return false;
      if (term && !`${s.code} ${s.name}`.toLowerCase().includes(term)) return false;
      return true;
    });
  }, [inventorySuppliers, supplierSearch, supplierStatusFilter]);

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

  // ── Non-blocked suppliers for PO creation ───────────────────────────────────
  const nonBlockedSuppliers = useMemo(
    () => inventorySuppliers.filter((s) => s.status_code !== "BLOCKED"),
    [inventorySuppliers],
  );

  // ── Requisition form JSX ────────────────────────────────────────────────────
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
        <Label>{t("procurement.requisition.priority")}</Label>
        <Select value={reqPriority} onValueChange={setReqPriority}>
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PURCHASE_PRIORITIES.map((p) => (
              <SelectItem key={p} value={p}>
                {t(`procurement.requisition.priorityValues.${p}`, { defaultValue: p })}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
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
        <Label>Demand source ref (optional)</Label>
        <Input value={reqSourceRef} onChange={(e) => setReqSourceRef(e.target.value)} />
      </div>
      <div className="space-y-1 md:col-span-2">
        <Label>Reason</Label>
        <Input value={reqReason} onChange={(e) => setReqReason(e.target.value)} />
      </div>
    </div>
  );

  // ── Receive form JSX ────────────────────────────────────────────────────────
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
                {line.article_code} — ordered {line.ordered_qty}
                {line.demand_source_ref ? ` · ${line.demand_source_ref}` : ""}
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
      {/* Fulfillment action — only relevant when PO line has a demand source */}
      {poLines.find((l) => l.id === receiptPoLineId)?.demand_source_ref ? (
        <div className="space-y-1">
          <Label>{t("procurement.receive.fulfillmentAction")}</Label>
          <Select value={receiptFulfillmentAction} onValueChange={setReceiptFulfillmentAction}>
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FULFILLMENT_ACTIONS.map((fa) => (
                <SelectItem key={fa} value={fa}>
                  {t(`procurement.receive.fulfillmentValues.${fa}`, { defaultValue: fa })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ) : null}
    </div>
  );

  const selectedSupplier = useMemo(
    () => inventorySuppliers.find((s) => s.id === selectedSupplierId) ?? null,
    [inventorySuppliers, selectedSupplierId],
  );

  // ── Dashboard KPI strip ─────────────────────────────────────────────────────
  const kpiStrip = dashboard ? (
    <div className="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-4 lg:grid-cols-5 xl:grid-cols-10">
      {(
        [
          {
            label: t("procurement.dashboard.openRequisitions"),
            value: dashboard.open_requisitions,
            onClick: () => {
              setActiveTab("requisitions");
              setReqStatusFilter("__all__");
            },
          },
          {
            label: t("procurement.dashboard.openPos"),
            value: dashboard.open_pos,
            onClick: () => setActiveTab("purchase-orders"),
          },
          {
            label: t("procurement.dashboard.latePos"),
            value: dashboard.overdue_pos,
            alert: dashboard.overdue_pos > 0,
            onClick: () => setActiveTab("purchase-orders"),
          },
          {
            label: t("procurement.dashboard.receivingToday", {
              defaultValue: "Receiving today",
            }),
            value: dashboard.receiving_today_count,
            onClick: () => setActiveTab("purchase-orders"),
          },
          {
            label: t("procurement.dashboard.pendingApprovals"),
            value: dashboard.pending_approval_pos,
            onClick: () => {
              setActiveTab("purchase-orders");
              setPoStatusFilter("SUBMITTED");
            },
          },
          {
            label: t("procurement.dashboard.stockouts"),
            value: dashboard.low_stock_articles,
            alert: dashboard.low_stock_articles > 0,
          },
          {
            label: t("procurement.dashboard.criticalStockouts"),
            value: dashboard.critical_low_stock_articles,
            alert: dashboard.critical_low_stock_articles > 0,
          },
          {
            label: t("procurement.dashboard.activeSuppliers", {
              defaultValue: "Active suppliers",
            }),
            value: dashboard.active_suppliers_count,
            onClick: () => setActiveTab("suppliers"),
          },
          {
            label: t("procurement.dashboard.repairablesInRepair", {
              defaultValue: "Repairables in repair",
            }),
            value: dashboard.repairables_in_repair,
            onClick: () => setActiveTab("repairables"),
          },
          {
            label: t("procurement.dashboard.pendingReceipts"),
            value: dashboard.pending_receipts,
            onClick: () => setActiveTab("purchase-orders"),
          },
        ] as Array<{ label: string; value: number; alert?: boolean; onClick?: () => void }>
      ).map((kpi) => (
        <button
          key={kpi.label}
          type="button"
          onClick={kpi.onClick}
          className={cn(
            "rounded-md border p-2.5 text-left transition",
            kpi.onClick ? "cursor-pointer hover:bg-muted" : "cursor-default",
            kpi.alert && kpi.value > 0 ? "border-status-warning/40 bg-status-warning/5" : "",
          )}
        >
          <div
            className={cn(
              "text-lg font-bold tabular-nums",
              kpi.alert && kpi.value > 0 ? "text-status-warning" : "",
            )}
          >
            {kpi.value}
          </div>
          <div className="mt-0.5 text-[11px] text-text-muted">{kpi.label}</div>
        </button>
      ))}
    </div>
  ) : null;

  return (
    <div className="space-y-4">
      {error ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2 text-sm">
          {error}
        </div>
      ) : null}

      {kpiStrip}

      <ProcurementAlertCards
        alerts={alerts}
        onNavigate={(tab) => {
          setActiveTab(tab);
          if (tab === "purchase-orders") setPoStatusFilter("__all__");
          if (tab === "repairables") setRepairStatusFilter("__all__");
        }}
      />

      <ProcurementRecommendationsPanel
        onCreateRequisitionPrefill={(rec) => {
          setReqArticleId(rec.article_id);
          setReqQty(rec.suggested_reorder_qty);
          setReqSourceType("REORDER");
          setReqSourceRef(rec.warehouse_code);
          setReqReason(rec.reason ?? "");
          setActiveTab("requisitions");
          setCreateReqOpen(true);
        }}
        onCreated={() => loadAll()}
      />

      <div className="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
        {/* Sub-tab sidebar */}
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
          {/* ── REQUISITIONS ── */}
          {activeTab === "requisitions" ? (
            <div className="rounded-md border p-4">
              <h3 className="mb-2 text-sm font-semibold">Requisitions</h3>
              <div className="mb-2 flex flex-wrap items-center gap-2">
                <Select value={reqStatusFilter} onValueChange={setReqStatusFilter}>
                  <SelectTrigger className="h-8 w-[180px] text-sm">
                    <SelectValue placeholder="Status" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All statuses</SelectItem>
                    {Array.from(new Set(requisitions.map((r) => r.status))).map((status) => (
                      <SelectItem key={status} value={status}>
                        {status}
                      </SelectItem>
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
                      <div
                        key={status}
                        className="min-w-[240px] flex-shrink-0 rounded-md border bg-muted/30 p-2"
                      >
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
                              {req.purchase_priority ? (
                                <div className="mt-0.5 text-[10px] font-medium text-status-warning">
                                  {req.purchase_priority}
                                </div>
                              ) : null}
                              <div className="mt-0.5 text-[11px] text-text-muted">
                                {req.demand_source_type}
                              </div>
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

          {/* ── PURCHASE ORDERS ── */}
          {activeTab === "purchase-orders" ? (
              <div className="rounded-md border p-4">
                <h3 className="mb-3 text-sm font-semibold">Purchase orders</h3>
                <div className="mb-3 flex flex-wrap gap-2">
                  <Select value={poStatusFilter} onValueChange={setPoStatusFilter}>
                    <SelectTrigger className="h-8 w-[180px] text-sm">
                      <SelectValue placeholder="Status" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__all__">All statuses</SelectItem>
                      {Array.from(new Set(purchaseOrders.map((po) => po.status))).map((status) => (
                        <SelectItem key={status} value={status}>
                          {status}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                {reqView === "kanban" ? (
                  <div className="mb-3 flex gap-3 overflow-x-auto pb-2">
                    {PO_KANBAN_STATUSES.map((status) => {
                      const rows = filteredPurchaseOrders.filter((row) => row.status === status);
                      return (
                        <div
                          key={status}
                          className="min-w-[240px] flex-shrink-0 rounded-md border bg-muted/30 p-2"
                        >
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
                                <div className="mt-1 text-[11px] text-text-muted">
                                  {po.supplier_company_name ?? "No supplier"}
                                </div>
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
          ) : null}

          {/* ── REPAIRABLES ── */}
          {activeTab === "repairables" ? (
              <div className="rounded-md border p-4">
                <h3 className="mb-2 text-sm font-semibold">Repairables</h3>
                <div className="mb-3 flex flex-wrap gap-2">
                  <Select value={repairStatusFilter} onValueChange={setRepairStatusFilter}>
                    <SelectTrigger className="h-8 w-[180px] text-sm">
                      <SelectValue placeholder="Status" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="__all__">All statuses</SelectItem>
                      {Array.from(new Set(repairables.map((order) => order.status))).map(
                        (status) => (
                          <SelectItem key={status} value={status}>
                            {status}
                          </SelectItem>
                        ),
                      )}
                    </SelectContent>
                  </Select>
                </div>
                <DataTable
                  columns={repairColumns}
                  data={filteredRepairables}
                  isLoading={loading}
                  searchable={false}
                  onRowClick={(order) => openRepairableDetails(order.id)}
                />
                <div className="mt-3 space-y-2 text-xs">
                  {filteredRepairables.length === 0 ? (
                    <p className="text-sm text-text-muted">No repairable orders yet.</p>
                  ) : null}
                  {filteredRepairables.slice(0, 8).map((order) => (
                    <div key={order.id} className="flex flex-wrap items-center gap-2 rounded border p-2">
                      <span>{order.order_code}</span>
                      <span>{order.status}</span>
                      <PermissionGate permission="inv.procure">
                        <Button
                        variant="outline"
                        size="sm"
                        disabled={saving || order.status !== "REQUESTED"}
                        onClick={() =>
                          void runSaving(async () => {
                            await transitionInventoryRepairableOrder({
                              order_id: order.id,
                              expected_row_version: order.row_version,
                              next_status: "RELEASED",
                            });
                          })
                        }
                      >
                        Release
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={saving || order.status !== "RELEASED"}
                        onClick={() =>
                          void runSaving(async () => {
                            await transitionInventoryRepairableOrder({
                              order_id: order.id,
                              expected_row_version: order.row_version,
                              next_status: "SENT_FOR_REPAIR",
                            });
                          })
                        }
                      >
                        Send
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={saving || order.status !== "SENT_FOR_REPAIR"}
                        onClick={() =>
                          void runSaving(async () => {
                            await transitionInventoryRepairableOrder({
                              order_id: order.id,
                              expected_row_version: order.row_version,
                              next_status: "RETURNED_FROM_REPAIR",
                              return_location_id: order.return_location_id,
                            });
                          })
                        }
                      >
                        Receive back
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={saving || order.status !== "RETURNED_FROM_REPAIR"}
                        onClick={() =>
                          void runSaving(async () => {
                            await transitionInventoryRepairableOrder({
                              order_id: order.id,
                              expected_row_version: order.row_version,
                              next_status: "CLOSED",
                            });
                          })
                        }
                      >
                        Close
                      </Button>
                      </PermissionGate>
                    </div>
                  ))}
                </div>
              </div>
          ) : null}

          {/* ── SUPPLIERS ── */}
          {activeTab === "suppliers" ? (
              <div className="rounded-md border p-4">
                <div className="mb-3 flex items-center justify-between">
                  <h3 className="text-sm font-semibold">{t("procurement.suppliers.title")}</h3>
                  <PermissionGate permission="inv.manage">
                    <Button size="sm" onClick={openNewSupplierForm}>
                      {t("procurement.suppliers.new")}
                    </Button>
                  </PermissionGate>
                </div>

                <SmartFilterBar
                  searchPlaceholder={t("procurement.suppliers.search")}
                  searchValue={supplierSearchInput}
                  onSearchInputChange={setSupplierSearchInput}
                  onSearchChange={(q) => setSupplierSearch(q.trim())}
                  filters={[
                    {
                  id: "status",
                    kind: "select",
                    label: t("procurement.suppliers.columns.status"),
                    value: supplierStatusFilter,
                    onChange: (v) => setSupplierStatusFilter(v ?? "__all__"),
                    options: SUPPLIER_STATUSES.map((s) => ({
                      value: s,
                      label: t(`procurement.suppliers.status.${s}`, { defaultValue: s }),
                    })),
                    },
                  ]}
                  resultCount={filteredInventorySuppliers.length}
                  onReset={() => {
                    setSupplierSearchInput("");
                    setSupplierSearch("");
                    setSupplierStatusFilter("__all__");
                  }}
                />

                <div className="mt-3 grid gap-4 lg:grid-cols-[1fr_380px]">
                  <DataTable
                    columns={supplierColumns}
                    data={filteredInventorySuppliers}
                    isLoading={loading}
                    searchable={false}
                    onRowClick={(s) => setSelectedSupplierId(s.id)}
                  />

                  {selectedSupplierId ? (
                    <div className="space-y-4">
                      {/* Supplier header */}
                      <div className="flex items-start justify-between rounded-md border p-3">
                        <div>
                          <div className="font-semibold">
                            {selectedSupplier?.code} — {selectedSupplier?.name}
                          </div>
                          {selectedSupplier ? (
                            <Badge
                              variant={statusBadgeVariant(selectedSupplier.status_code)}
                              className="mt-1 text-[10px]"
                            >
                              {t(`procurement.suppliers.status.${selectedSupplier.status_code}`, {
                                defaultValue: selectedSupplier.status_code,
                              })}
                            </Badge>
                          ) : null}
                        </div>
                        <PermissionGate permission="inv.manage">
                          <div className="flex gap-2">
                            {selectedSupplier ? (
                              <>
                                <Button
                                  size="sm"
                                  variant="outline"
                                  onClick={() => openEditSupplierForm(selectedSupplier)}
                                >
                                  Edit
                                </Button>
                                {selectedSupplier.is_active === 1 ? (
                                  <Button
                                    size="sm"
                                    variant="destructive"
                                    disabled={saving}
                                    onClick={() =>
                                      void runSaving(async () => {
                                        await deactivateInventorySupplier(
                                          selectedSupplier.id,
                                          selectedSupplier.row_version,
                                        );
                                        setSelectedSupplierId(null);
                                      })
                                    }
                                  >
                                    Deactivate
                                  </Button>
                                ) : null}
                              </>
                            ) : null}
                          </div>
                        </PermissionGate>
                      </div>

                      {/* Scorecard */}
                      {supplierScorecardLoading ? (
                        <div className="rounded-md border p-3 text-sm text-text-muted">
                          Loading…
                        </div>
                      ) : supplierScorecard ? (
                        <div className="rounded-md border p-3">
                          <div className="mb-2 text-xs font-semibold">
                            {t("procurement.suppliers.scorecard.title")}
                          </div>
                          <div className="grid grid-cols-2 gap-1.5 text-xs">
                            {(
                              [
                                [
                                  t("procurement.suppliers.scorecard.otif"),
                                  pctLabel(supplierScorecard.on_time_delivery_pct),
                                ],
                                [
                                  t("procurement.suppliers.scorecard.avgLeadTime"),
                                  numLabel(supplierScorecard.avg_lead_time_days),
                                ],
                                [
                                  t("procurement.suppliers.scorecard.deliveryAccuracy"),
                                  pctLabel(supplierScorecard.delivery_accuracy_pct),
                                ],
                                [
                                  t("procurement.suppliers.scorecard.avgPrice"),
                                  numLabel(supplierScorecard.avg_price, 2),
                                ],
                                [
                                  t("procurement.suppliers.scorecard.openPos"),
                                  String(supplierScorecard.open_po_count),
                                ],
                                [
                                  t("procurement.suppliers.scorecard.completedPos"),
                                  String(supplierScorecard.completed_po_count),
                                ],
                                [
                                  t("procurement.suppliers.scorecard.lastPurchase"),
                                  supplierScorecard.last_purchase_at
                                    ? new Date(supplierScorecard.last_purchase_at).toLocaleDateString()
                                    : "—",
                                ],
                                [
                                  t("procurement.suppliers.scorecard.rejectedPct"),
                                  pctLabel(supplierScorecard.rejected_pct),
                                ],
                              ] as [string, string][]
                            ).map(([label, value]) => (
                              <div key={label} className="flex justify-between gap-2">
                                <span className="text-text-muted">{label}</span>
                                <span className="font-medium tabular-nums">{value}</span>
                              </div>
                            ))}
                            <div className="flex items-center justify-between gap-2">
                              <span className="text-text-muted">
                                {t("procurement.suppliers.scorecard.risk")}
                              </span>
                              <Badge
                                variant={riskBadgeVariant(supplierScorecard.risk_level)}
                                className="h-4 text-[9px]"
                              >
                                {t(`procurement.suppliers.risk.${supplierScorecard.risk_level}`, {
                                  defaultValue: supplierScorecard.risk_level,
                                })}
                              </Badge>
                            </div>
                          </div>

                          <div className="mt-3">
                            <div className="mb-1.5 text-xs font-semibold">
                              {t("procurement.suppliers.scorecard.lastDeliveries")}
                            </div>
                            {supplierScorecard.last_deliveries.length === 0 ? (
                              <p className="text-xs text-text-muted">
                                {t("procurement.suppliers.scorecard.lastDeliveriesEmpty")}
                              </p>
                            ) : (
                              <Table>
                                <TableHeader>
                                  <TableRow>
                                    <TableHead className="h-7 px-2 text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.receivedAt")}
                                    </TableHead>
                                    <TableHead className="h-7 px-2 text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.po")}
                                    </TableHead>
                                    <TableHead className="h-7 px-2 text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.article")}
                                    </TableHead>
                                    <TableHead className="h-7 px-2 text-right text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.orderedQty")}
                                    </TableHead>
                                    <TableHead className="h-7 px-2 text-right text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.acceptedQty")}
                                    </TableHead>
                                    <TableHead className="h-7 px-2 text-right text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.rejectedQty")}
                                    </TableHead>
                                    <TableHead className="h-7 px-2 text-right text-xs">
                                      {t("procurement.suppliers.scorecard.deliveryColumns.leadTime")}
                                    </TableHead>
                                  </TableRow>
                                </TableHeader>
                                <TableBody>
                                  {supplierScorecard.last_deliveries.map((delivery, index) => (
                                    <TableRow key={`${delivery.po_number}-${delivery.article_code}-${index}`}>
                                      <TableCell className="px-2 py-1.5 text-xs">
                                        {delivery.received_at
                                          ? new Date(delivery.received_at).toLocaleDateString()
                                          : "—"}
                                      </TableCell>
                                      <TableCell className="px-2 py-1.5 font-mono text-xs">
                                        {formatEntityCode(delivery.po_number)}
                                      </TableCell>
                                      <TableCell className="px-2 py-1.5 text-xs">
                                        {formatAssetLabel(delivery.article_code, delivery.article_name)}
                                      </TableCell>
                                      <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                        {numLabel(delivery.ordered_qty)}
                                      </TableCell>
                                      <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                        {numLabel(delivery.accepted_qty)}
                                      </TableCell>
                                      <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                        {numLabel(delivery.rejected_qty)}
                                      </TableCell>
                                      <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                        {numLabel(delivery.actual_lead_time_days)}
                                      </TableCell>
                                    </TableRow>
                                  ))}
                                </TableBody>
                              </Table>
                            )}
                          </div>
                        </div>
                      ) : null}

                      <SupplierArticleSourcesSection
                        supplierId={selectedSupplierId}
                        articles={articles}
                        onChanged={() => void loadAll()}
                      />

                      <SupplierContactsSection supplierId={selectedSupplierId} />

                      {/* Purchase history */}
                      <div className="rounded-md border p-3">
                        <div className="mb-2 text-xs font-semibold">
                          {t("procurement.suppliers.purchaseHistory.title")}
                        </div>
                        {supplierPurchaseHistory.length === 0 ? (
                          <p className="text-xs text-text-muted">
                            {t("procurement.suppliers.purchaseHistory.empty")}
                          </p>
                        ) : (
                          <Table>
                            <TableHeader>
                              <TableRow>
                                <TableHead className="h-7 px-2 text-xs">
                                  {t("procurement.suppliers.purchaseHistory.columns.orderedAt")}
                                </TableHead>
                                <TableHead className="h-7 px-2 text-xs">
                                  {t("procurement.suppliers.purchaseHistory.columns.po")}
                                </TableHead>
                                <TableHead className="h-7 px-2 text-xs">
                                  {t("procurement.suppliers.purchaseHistory.columns.article")}
                                </TableHead>
                                <TableHead className="h-7 px-2 text-right text-xs">
                                  {t("procurement.suppliers.purchaseHistory.columns.orderedQty")}
                                </TableHead>
                                <TableHead className="h-7 px-2 text-right text-xs">
                                  {t("procurement.suppliers.purchaseHistory.columns.unitPrice")}
                                </TableHead>
                                <TableHead className="h-7 px-2 text-xs">
                                  {t("procurement.suppliers.purchaseHistory.columns.status")}
                                </TableHead>
                              </TableRow>
                            </TableHeader>
                            <TableBody>
                              {supplierPurchaseHistory.map((row) => (
                                <TableRow key={`${row.purchase_order_id}-${row.article_id}`}>
                                  <TableCell className="px-2 py-1.5 text-xs">
                                    {row.ordered_at
                                      ? new Date(row.ordered_at).toLocaleDateString()
                                      : "—"}
                                  </TableCell>
                                  <TableCell className="px-2 py-1.5 text-xs">
                                    <button
                                      type="button"
                                      className="font-mono font-medium text-primary hover:underline"
                                      onClick={() => openPoDetails(row.purchase_order_id)}
                                    >
                                      {formatEntityCode(row.po_number)}
                                    </button>
                                  </TableCell>
                                  <TableCell className="px-2 py-1.5 text-xs">
                                    {formatAssetLabel(row.article_code, row.article_name)}
                                  </TableCell>
                                  <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                    {numLabel(row.ordered_qty)}
                                  </TableCell>
                                  <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                    {numLabel(row.unit_price, 2)}
                                  </TableCell>
                                  <TableCell className="px-2 py-1.5 text-xs">
                                    <Badge variant="outline" className="h-4 text-[9px]">
                                      {row.status}
                                    </Badge>
                                  </TableCell>
                                </TableRow>
                              ))}
                            </TableBody>
                          </Table>
                        )}
                      </div>

                      {/* Price list */}
                      <div className="rounded-md border p-3">
                        <div className="mb-2 flex items-center justify-between">
                          <div className="text-xs font-semibold">
                            {t("procurement.suppliers.prices.title")}
                          </div>
                          <PermissionGate permission="inv.manage">
                            <Button
                              size="sm"
                              variant="outline"
                              onClick={() => {
                                setPriceFormArticleId(0);
                                setPriceFormUnitPrice("");
                                setPriceFormMinQty("");
                                setPriceFormValidFrom("");
                                setPriceFormOpen(true);
                              }}
                            >
                              {t("procurement.suppliers.prices.new")}
                            </Button>
                          </PermissionGate>
                        </div>
                        {supplierPrices.length === 0 ? (
                          <p className="text-xs text-text-muted">
                            {t("procurement.suppliers.prices.empty")}
                          </p>
                        ) : (
                          <div className="space-y-1.5 text-xs">
                            {supplierPrices.map((price) => (
                              <div
                                key={price.id}
                                className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-1.5"
                              >
                                <span className="font-medium">
                                  {price.article_code} — {price.article_name}
                                </span>
                                <span className="tabular-nums">{price.unit_price.toFixed(2)}</span>
                                {price.min_order_qty ? (
                                  <span className="text-text-muted">
                                    min {price.min_order_qty}
                                  </span>
                                ) : null}
                                {price.valid_from ? (
                                  <span className="text-text-muted">
                                    from{" "}
                                    {new Date(price.valid_from).toLocaleDateString()}
                                  </span>
                                ) : null}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>

                      {/* Document links */}
                      <div className="rounded-md border p-3">
                        <div className="mb-2 flex items-center justify-between">
                          <div className="text-xs font-semibold">
                            {t("procurement.suppliers.documents.title")}
                          </div>
                          <PermissionGate permission="inv.manage">
                            <Button
                              size="sm"
                              variant="outline"
                              onClick={() => {
                                setDocLinkFormRef("");
                                setDocLinkFormPurpose("CONTRACT");
                                setDocLinkFormPrimary(false);
                                setDocLinkFormOpen(true);
                              }}
                            >
                              {t("procurement.suppliers.documents.new")}
                            </Button>
                          </PermissionGate>
                        </div>
                        {supplierDocs.length === 0 ? (
                          <p className="text-xs text-text-muted">
                            {t("procurement.suppliers.documents.empty")}
                          </p>
                        ) : (
                          <div className="space-y-1.5 text-xs">
                            {supplierDocs.map((doc) => (
                              <div
                                key={doc.id}
                                className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-1.5"
                              >
                                <span className="break-all font-medium">{doc.document_ref}</span>
                                <span className="text-text-muted">{doc.link_purpose}</span>
                                {doc.is_primary === 1 ? (
                                  <Badge variant="secondary" className="h-4 text-[9px]">
                                    Primary
                                  </Badge>
                                ) : null}
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  ) : null}
                </div>
              </div>
          ) : null}

          {/* ── LIFECYCLE EVENTS ── */}
          {activeTab === "lifecycle" ? (
            <div className="rounded-md border p-4">
              <h3 className="mb-2 text-sm font-semibold">Latest lifecycle events</h3>
              <div className="mb-3 grid gap-2 md:grid-cols-4">
                <Select value={eventArticleFilter} onValueChange={setEventArticleFilter}>
                  <SelectTrigger className="h-8 text-sm">
                    <SelectValue placeholder="Article" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All articles</SelectItem>
                    {articles.map((a) => (
                      <SelectItem key={a.id} value={String(a.id)}>
                        {a.article_code}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Select value={eventWarehouseFilter} onValueChange={setEventWarehouseFilter}>
                  <SelectTrigger className="h-8 text-sm">
                    <SelectValue placeholder="Warehouse" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All warehouses</SelectItem>
                    {warehouseOptions.map((w) => (
                      <SelectItem key={w} value={w}>
                        {w}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Input
                  type="date"
                  value={eventFromDate}
                  onChange={(e) => setEventFromDate(e.target.value)}
                />
                <Input
                  type="date"
                  value={eventToDate}
                  onChange={(e) => setEventToDate(e.target.value)}
                />
              </div>
              <DataTable
                columns={eventColumns}
                data={filteredEvents}
                isLoading={loading}
                searchable={false}
              />
            </div>
          ) : null}
        </div>
      </div>

      {/* ── DIALOGS ── */}
        {/* Requisition detail dialog */}
        <Dialog open={reqDetailOpen} onOpenChange={setReqDetailOpen}>
          <DialogContent className="max-h-[85vh] max-w-xl overflow-y-auto">
            <DialogHeader>
              <DialogTitle>
                {selectedRequisition
                  ? `${selectedRequisition.req_number} — ${selectedRequisition.status}`
                  : "Request details"}
              </DialogTitle>
            </DialogHeader>
            {selectedRequisition ? (
              <div className="space-y-3">
                <div className="rounded-md border p-3 text-sm">
                  <div>
                    <span className="text-text-muted">Source type:</span>{" "}
                    {selectedRequisition.demand_source_type}
                  </div>
                  <div>
                    <span className="text-text-muted">Source ref:</span>{" "}
                    {selectedRequisition.demand_source_ref ?? "—"}
                  </div>
                  {selectedRequisition.purchase_priority ? (
                    <div>
                      <span className="text-text-muted">
                        {t("procurement.requisition.priority")}:
                      </span>{" "}
                      <span className="font-medium">
                        {t(
                          `procurement.requisition.priorityValues.${selectedRequisition.purchase_priority}`,
                          { defaultValue: selectedRequisition.purchase_priority },
                        )}
                      </span>
                    </div>
                  ) : null}
                  <div>
                    <span className="text-text-muted">Created:</span> {selectedRequisition.created_at}
                  </div>
                  <div>
                    <span className="text-text-muted">Updated:</span> {selectedRequisition.updated_at}
                  </div>
                </div>
                <div className="rounded-md border p-3 text-sm">
                  <div className="mb-2 font-medium">Lines</div>
                  {reqLines.length === 0 ? (
                    <div className="text-text-muted">No lines</div>
                  ) : (
                    <div className="space-y-1">
                      {reqLines.map((line) => (
                        <div key={line.id}>
                          {line.article_code} — {line.article_name} × {line.requested_qty}
                          {line.preferred_location_code ? (
                            <span className="ml-2 text-xs text-text-muted">
                              → {line.preferred_location_code}
                            </span>
                          ) : null}
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
                <PermissionGate permission="inv.procure">
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
                  {(() => {
                    const target = requisitionDeclineOrCloseTarget(selectedRequisition.status);
                    if (!target) return null;
                    const label =
                      selectedRequisition.status === "APPROVED" ||
                      selectedRequisition.status === "PARTIALLY_RECEIVED"
                        ? "Close"
                        : "Cancel";
                    return (
                      <Button
                        variant="destructive"
                        disabled={saving}
                        onClick={() => void transitionSelectedRequisition(target)}
                      >
                        {label}
                      </Button>
                    );
                  })()}
                  </>
                </PermissionGate>
              ) : null}
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* PO detail workspace */}
        <PurchaseOrderWorkspace
          open={poDetailOpen}
          onOpenChange={setPoDetailOpen}
          poId={selectedPoId}
          onTransitioned={() => {
            void loadAll();
          }}
          onReload={loadAll}
        />

        <RepairableDetailDialog
          open={repairDetailOpen}
          onOpenChange={setRepairDetailOpen}
          orderId={selectedRepairableId}
          onTransitioned={() => {
            void loadAll();
          }}
        />

        {/* Create requisition dialog */}
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
                      demand_source_ref: reqSourceRef.trim() || null,
                      source_reservation_id: reqReservationId.trim()
                        ? Number(reqReservationId)
                        : null,
                      source_reorder_trigger: reqSourceType === "REORDER" ? "threshold_crossed" : null,
                      purchase_priority: reqPriority || null,
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

        {/* Create PO dialog */}
        <Dialog open={createPoOpen} onOpenChange={setCreatePoOpen}>
          <DialogContent
            className="max-w-lg"
            onPointerDownOutside={(e) => e.preventDefault()}
          >
            <DialogHeader>
              <DialogTitle>Create purchase order</DialogTitle>
            </DialogHeader>
            <div className="grid gap-3 py-2">
              <div className="space-y-1">
                <Label>Approved requisition</Label>
                <Select
                  value={String(poDialogReqId)}
                  onValueChange={(v) => setPoDialogReqId(Number(v))}
                >
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
                <Select
                  value={String(poSupplierId)}
                  onValueChange={(v) => setPoSupplierId(Number(v))}
                >
                  <SelectTrigger>
                    <SelectValue placeholder="Supplier" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">No supplier</SelectItem>
                    {nonBlockedSuppliers.map((s) => (
                      <SelectItem key={s.id} value={String(s.id)}>
                        {s.code} — {s.name}
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
                      supplier_id: poSupplierId > 0 ? poSupplierId : null,
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

        {/* Receive goods dialog */}
        <Dialog open={receiveOpen} onOpenChange={setReceiveOpen}>
          <DialogContent
            className="max-w-lg"
            onPointerDownOutside={(e) => e.preventDefault()}
          >
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
                disabled={
                  saving ||
                  !selectedPo ||
                  receiptPoLineId <= 0 ||
                  receiptLocationId <= 0 ||
                  receiptQty <= 0
                }
                onClick={() =>
                  void runSaving(async () => {
                    const line = poLines.find((item) => item.id === receiptPoLineId);
                    if (!line || !selectedPo) return;
                    const hasDemand = Boolean(line.demand_source_ref);
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
                      fulfillment_action: hasDemand ? receiptFulfillmentAction : null,
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

        {/* Repairable order dialog */}
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
                <Select
                  value={String(repairArticleId)}
                  onValueChange={(v) => setRepairArticleId(Number(v))}
                >
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
                <Select
                  value={String(repairSourceLocationId)}
                  onValueChange={(v) => setRepairSourceLocationId(Number(v))}
                >
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
                <Select
                  value={String(repairReturnLocationId)}
                  onValueChange={(v) => setRepairReturnLocationId(Number(v))}
                >
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
                        {b.warehouse_code}/{b.location_code}: on-hand {b.on_hand_qty}, reserved{" "}
                        {b.reserved_qty}, available {b.available_qty}
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

        {/* Supplier create/edit dialog */}
        <Dialog open={supplierEditOpen} onOpenChange={setSupplierEditOpen}>
          <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>
                {supplierForm.id
                  ? t("procurement.suppliers.edit")
                  : t("procurement.suppliers.new")}
              </DialogTitle>
            </DialogHeader>
            <div className="grid gap-3 md:grid-cols-2">
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.fields.code")}</Label>
                <Input
                  value={supplierForm.code}
                  onChange={(e) => setSupplierForm((s) => ({ ...s, code: e.target.value }))}
                />
              </div>
              <div className="space-y-1 md:col-span-1">
                <Label>{t("procurement.suppliers.fields.name")}</Label>
                <Input
                  value={supplierForm.name}
                  onChange={(e) => setSupplierForm((s) => ({ ...s, name: e.target.value }))}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.fields.status")}</Label>
                <Select
                  value={supplierForm.statusCode}
                  onValueChange={(v) => setSupplierForm((s) => ({ ...s, statusCode: v }))}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {SUPPLIER_STATUSES.map((status) => (
                      <SelectItem key={status} value={status}>
                        {t(`procurement.suppliers.status.${status}`, { defaultValue: status })}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.fields.leadTime")}</Label>
                <Input
                  type="number"
                  min={0}
                  value={supplierForm.defaultLeadTimeDays}
                  onChange={(e) =>
                    setSupplierForm((s) => ({ ...s, defaultLeadTimeDays: e.target.value }))
                  }
                />
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.fields.paymentTerms")}</Label>
                <Input
                  value={supplierForm.paymentTermsCode}
                  onChange={(e) =>
                    setSupplierForm((s) => ({ ...s, paymentTermsCode: e.target.value }))
                  }
                />
              </div>
              <div className="flex items-center gap-2 pt-4">
                <input
                  type="checkbox"
                  id="supplier-active"
                  checked={supplierForm.isActive}
                  onChange={(e) =>
                    setSupplierForm((s) => ({ ...s, isActive: e.target.checked }))
                  }
                />
                <Label htmlFor="supplier-active">{t("procurement.suppliers.fields.active")}</Label>
              </div>
            </div>
            <DialogFooter className="gap-2 sm:gap-0">
              <Button variant="outline" onClick={() => setSupplierEditOpen(false)}>
                Cancel
              </Button>
              <Button
                disabled={saving || !supplierForm.code.trim() || !supplierForm.name.trim()}
                onClick={() => void saveSupplierForm()}
              >
                Save
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* Supplier price form dialog */}
        <Dialog open={priceFormOpen} onOpenChange={setPriceFormOpen}>
          <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>{t("procurement.suppliers.prices.new")}</DialogTitle>
            </DialogHeader>
            <div className="grid gap-3">
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.prices.fields.article")}</Label>
                <Select
                  value={String(priceFormArticleId)}
                  onValueChange={(v) => setPriceFormArticleId(Number(v))}
                >
                  <SelectTrigger>
                    <SelectValue placeholder="Select article" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="0">Select article</SelectItem>
                    {articles.map((a) => (
                      <SelectItem key={a.id} value={String(a.id)}>
                        {a.article_code} — {a.article_name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.prices.fields.unitPrice")}</Label>
                <Input
                  type="number"
                  min={0}
                  step="0.01"
                  value={priceFormUnitPrice}
                  onChange={(e) => setPriceFormUnitPrice(e.target.value)}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.prices.fields.minQty")}</Label>
                <Input
                  type="number"
                  min={0}
                  step="0.01"
                  value={priceFormMinQty}
                  onChange={(e) => setPriceFormMinQty(e.target.value)}
                />
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.prices.fields.validFrom")}</Label>
                <Input
                  type="date"
                  value={priceFormValidFrom}
                  onChange={(e) => setPriceFormValidFrom(e.target.value)}
                />
              </div>
            </div>
            <DialogFooter className="gap-2 sm:gap-0">
              <Button variant="outline" onClick={() => setPriceFormOpen(false)}>
                Cancel
              </Button>
              <Button
                disabled={saving || priceFormArticleId <= 0 || !priceFormUnitPrice}
                onClick={() => void savePriceForm()}
              >
                Save
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>

        {/* Document link form dialog */}
        <Dialog open={docLinkFormOpen} onOpenChange={setDocLinkFormOpen}>
          <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>{t("procurement.suppliers.documents.new")}</DialogTitle>
            </DialogHeader>
            <div className="grid gap-3">
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.documents.fields.ref")}</Label>
                <Input
                  value={docLinkFormRef}
                  onChange={(e) => setDocLinkFormRef(e.target.value)}
                  placeholder="e.g. contract-2024.pdf or https://…"
                />
              </div>
              <div className="space-y-1">
                <Label>{t("procurement.suppliers.documents.fields.purpose")}</Label>
                <Select value={docLinkFormPurpose} onValueChange={setDocLinkFormPurpose}>
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="CONTRACT">Contract</SelectItem>
                    <SelectItem value="CERTIFICATE">Certificate</SelectItem>
                    <SelectItem value="DATASHEET">Datasheet</SelectItem>
                    <SelectItem value="OTHER">Other</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              <div className="flex items-center gap-2">
                <input
                  type="checkbox"
                  id="doc-link-primary"
                  checked={docLinkFormPrimary}
                  onChange={(e) => setDocLinkFormPrimary(e.target.checked)}
                />
                <Label htmlFor="doc-link-primary">
                  {t("procurement.suppliers.documents.fields.primary")}
                </Label>
              </div>
            </div>
            <DialogFooter className="gap-2 sm:gap-0">
              <Button variant="outline" onClick={() => setDocLinkFormOpen(false)}>
                Cancel
              </Button>
              <Button
                disabled={saving || !docLinkFormRef.trim()}
                onClick={() => void saveDocLinkForm()}
              >
                Save
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
    </div>
  );
});
