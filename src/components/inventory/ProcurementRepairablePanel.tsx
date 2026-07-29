import type { ColumnDef } from "@tanstack/react-table";
import { Columns3, List, MoreHorizontal } from "lucide-react";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { ProcurementAlertCards } from "@/components/inventory/ProcurementAlertCards";
import { ProcurementContextMenu } from "@/components/inventory/ProcurementContextMenu";
import type { ProcurementContextMenuAction } from "@/components/inventory/ProcurementContextMenu";
import { ProcurementRecommendationsPanel } from "@/components/inventory/ProcurementRecommendationsPanel";
import { PurchaseOrderWorkspace } from "@/components/inventory/PurchaseOrderWorkspace";
import {
  ReceiveGoodsDialog,
  isReceivablePurchaseOrder,
} from "@/components/inventory/ReceiveGoodsDialog";
import {
  RepairableActionDialog,
  type RepairableActionKind,
  type RepairableActionPayload,
} from "@/components/inventory/RepairableActionDialog";
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
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import { usePermissions } from "@/hooks/use-permissions";
import { formatAssetLabel, formatEntityCode, formatOrDash } from "@/lib/display";
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
  listInventoryGoodsReceiptLines,
  listInventoryGoodsReceipts,
  listInventoryLocations,
  listInventoryProcurementRequisitionLines,
  listInventoryProcurementRequisitions,
  listInventoryPurchaseOrders,
  listInventoryRepairableOrders,
  listInventoryStateEvents,
  listInventoryStockBalances,
  listInventorySuppliers,
  listSupplierPrices,
  listSupplierPurchaseHistory,
  transitionInventoryProcurementRequisition,
  transitionInventoryPurchaseOrder,
  transitionInventoryRepairableOrder,
  upsertInventoryDocumentLink,
  upsertInventorySupplier,
  upsertSupplierPrice,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  GoodsReceipt,
  GoodsReceiptLine,
  InventoryArticle,
  InventoryDocumentLink,
  InventoryStateEvent,
  InventoryStockBalance,
  InventorySupplier,
  ProcurementAlert,
  ProcurementDashboardSummary,
  ProcurementRequisition,
  ProcurementRequisitionLine,
  PurchaseOrder,
  RepairableOrder,
  StockLocation,
  SupplierPrice,
  SupplierPurchaseHistoryRow,
  SupplierScorecard,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type ProcurementRepairablePanelHandle = {
  openCreateRequisition: () => void;
  openCreatePo: () => void;
  openReceiveGoods: () => void;
  openRepairable: () => void;
  /** Deep-link into a purchase order from another inventory surface. */
  openPurchaseOrder: (purchaseOrderId: number) => void;
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
  /** Navigate to an article master record (from repairable detail). */
  onOpenArticle?: (articleId: number) => void;
};

type SubTab =
  | "requisitions"
  | "purchase-orders"
  | "goods-receipts"
  | "repairables"
  | "suppliers"
  | "lifecycle";

const SUPPLIER_STATUSES = ["PREFERRED", "APPROVED", "BLOCKED", "UNDER_EVALUATION"] as const;

const PURCHASE_PRIORITIES = ["EMERGENCY", "HIGH", "NORMAL", "LOW"] as const;

const ALL = "__all__";

const REQ_KANBAN_STATUSES = [
  "DRAFT",
  "SUBMITTED",
  "APPROVED",
  "PARTIALLY_RECEIVED",
  "CLOSED",
  "REJECTED",
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

const REPAIRABLE_STATUSES = [
  "REQUESTED",
  "RELEASED",
  "SENT_FOR_REPAIR",
  "RETURNED_FROM_REPAIR",
  "CLOSED",
  "SCRAPPED",
  "CANCELLED",
] as const;

/** Repairable transitions that must capture extra input before posting. */
const REPAIRABLE_ACTION_TARGET: Record<RepairableActionKind, string> = {
  dispatch: "SENT_FOR_REPAIR",
  scrap: "SCRAPPED",
  cancel: "CANCELLED",
  receive: "RETURNED_FROM_REPAIR",
};

function pctLabel(v: number | null): string {
  if (v == null) return "—";
  return `${v.toFixed(1)}%`;
}

function numLabel(v: number | null, decimals = 1): string {
  if (v == null) return "—";
  return v.toFixed(decimals);
}

function statusBadgeVariant(status: string): "secondary" | "outline" | "destructive" | "default" {
  if (status === "PREFERRED") return "default";
  if (status === "BLOCKED") return "destructive";
  if (status === "APPROVED") return "secondary";
  return "outline";
}

/** Shared badge styling for procurement / repairable lifecycle statuses. */
function lifecycleBadgeVariant(
  status: string,
): "secondary" | "outline" | "destructive" | "default" {
  if (status === "REJECTED" || status === "CANCELLED" || status === "SCRAPPED")
    return "destructive";
  if (status === "APPROVED" || status === "RELEASED") return "default";
  if (status === "CLOSED" || status === "RECEIVED_CLOSED") return "secondary";
  return "outline";
}

function includesTerm(term: string, ...fields: Array<string | null | undefined>): boolean {
  if (!term) return true;
  return fields.some((field) => (field ?? "").toLowerCase().includes(term));
}

/** Overflow trigger for list rows — always renders the shared procurement menu. */
function RowActionsMenu({
  label,
  actions,
}: {
  label: string;
  actions: ProcurementContextMenuAction[];
}) {
  if (actions.length === 0) return null;
  return (
    <ProcurementContextMenu actions={actions}>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8"
        aria-label={label}
        onClick={(event) => event.stopPropagation()}
      >
        <MoreHorizontal className="h-4 w-4" aria-hidden />
      </Button>
    </ProcurementContextMenu>
  );
}

export const ProcurementRepairablePanel = forwardRef<
  ProcurementRepairablePanelHandle,
  ProcurementRepairablePanelProps
>(function ProcurementRepairablePanel(props, ref) {
  const { viewMode: viewModeProp, onViewModeChange, onOpenArticle } = props;
  const { t } = useTranslation("inventory");
  const { can } = usePermissions();
  const canProcure = can(P.INV_PROCURE);

  // ── Core state ─────────────────────────────────────────────────────────────
  const [activeTab, setActiveTab] = useState<SubTab>("requisitions");
  const [articles, setArticles] = useState<InventoryArticle[]>([]);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [inventorySuppliers, setInventorySuppliers] = useState<InventorySupplier[]>([]);
  const [requisitions, setRequisitions] = useState<ProcurementRequisition[]>([]);
  const [selectedReqId, setSelectedReqId] = useState<number | null>(null);
  const [reqLines, setReqLines] = useState<ProcurementRequisitionLine[]>([]);
  const [purchaseOrders, setPurchaseOrders] = useState<PurchaseOrder[]>([]);
  const [selectedPoId, setSelectedPoId] = useState<number | null>(null);
  const [repairables, setRepairables] = useState<RepairableOrder[]>([]);
  const [goodsReceipts, setGoodsReceipts] = useState<GoodsReceipt[]>([]);
  const [selectedGrId, setSelectedGrId] = useState<number | null>(null);
  const [grLines, setGrLines] = useState<GoodsReceiptLine[]>([]);
  const [grLinesLoading, setGrLinesLoading] = useState(false);
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
  const [reqSourceRef, setReqSourceRef] = useState<string>("");
  const [reqReservationId] = useState<string>("");
  const [reqReason, setReqReason] = useState<string>("");
  const [reqPriority, setReqPriority] = useState<string>("NORMAL");

  // ── PO form ─────────────────────────────────────────────────────────────────
  const [poSupplierId, setPoSupplierId] = useState<number>(0);
  const [poDialogReqId, setPoDialogReqId] = useState<number>(0);

  // ── Receive goods ───────────────────────────────────────────────────────────
  const [receiveInitialPoId, setReceiveInitialPoId] = useState<number | null>(null);
  /** Bumped after a receipt is posted so an open PO workspace refetches. */
  const [poReloadToken, setPoReloadToken] = useState(0);

  // ── Repairable form ─────────────────────────────────────────────────────────
  const [repairArticleId, setRepairArticleId] = useState<number>(0);
  const [repairQty, setRepairQty] = useState<number>(0);
  const [repairSourceLocationId, setRepairSourceLocationId] = useState<number>(0);
  const [repairReturnLocationId, setRepairReturnLocationId] = useState<number>(0);
  const [repairReason, setRepairReason] = useState<string>("");
  const [repairSerial, setRepairSerial] = useState<string>("");
  const [repairVendorId, setRepairVendorId] = useState<number>(0);
  const [repairBalances, setRepairBalances] = useState<InventoryStockBalance[]>([]);

  // ── Repairable lifecycle capture ────────────────────────────────────────────
  const [repairAction, setRepairAction] = useState<{
    kind: RepairableActionKind;
    order: RepairableOrder;
  } | null>(null);

  // ── Requisition rejection + history ─────────────────────────────────────────
  const [rejectOpen, setRejectOpen] = useState(false);
  const [rejectReason, setRejectReason] = useState("");
  const [reqEvents, setReqEvents] = useState<InventoryStateEvent[]>([]);
  const [reqEventsLoading, setReqEventsLoading] = useState(false);

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
  const [reqSearchInput, setReqSearchInput] = useState("");
  const [reqSearch, setReqSearch] = useState("");
  const [reqStatusFilter, setReqStatusFilter] = useState<string>(ALL);
  const [poSearchInput, setPoSearchInput] = useState("");
  const [poSearch, setPoSearch] = useState("");
  const [poStatusFilter, setPoStatusFilter] = useState<string>(ALL);
  const [poSupplierFilter, setPoSupplierFilter] = useState<string>(ALL);
  const [repairSearchInput, setRepairSearchInput] = useState("");
  const [repairSearch, setRepairSearch] = useState("");
  const [repairStatusFilter, setRepairStatusFilter] = useState<string>(ALL);
  const [repairVendorFilter, setRepairVendorFilter] = useState<string>(ALL);
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

  // ── Data loading ────────────────────────────────────────────────────────────
  const loadAll = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [
        articleRows,
        locationRows,
        inventorySupplierRows,
        requisitionRows,
        poRows,
        repairRows,
        goodsReceiptRows,
        eventRows,
        dashboardData,
        alertRows,
      ] = await Promise.all([
        listInventoryArticles({ search: null }),
        listInventoryLocations(null),
        listInventorySuppliers(),
        listInventoryProcurementRequisitions(),
        listInventoryPurchaseOrders(),
        listInventoryRepairableOrders(),
        listInventoryGoodsReceipts(),
        listInventoryStateEvents(undefined, undefined),
        getProcurementDashboardSummary(),
        getProcurementAlerts(),
      ]);
      setArticles(articleRows.filter((a) => a.is_active === 1));
      setLocations(locationRows.filter((l) => l.is_active === 1));
      setInventorySuppliers(inventorySupplierRows);
      setRequisitions(requisitionRows);
      setPurchaseOrders(poRows);
      setRepairables(repairRows);
      setGoodsReceipts(goodsReceiptRows);
      setStateEvents(eventRows.slice(0, 20));
      setDashboard(dashboardData);
      setAlerts(alertRows);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

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
      setReceiveInitialPoId(null);
      setReceiveOpen(true);
    },
    openRepairable: () => {
      setActiveTab("repairables");
      setRepairOpen(true);
    },
    openPurchaseOrder: (purchaseOrderId: number) => {
      setActiveTab("purchase-orders");
      openPoDetails(purchaseOrderId);
    },
    reload: () => loadAll(),
  }));

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

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
    if (!selectedGrId) {
      setGrLines([]);
      return;
    }
    let cancelled = false;
    setGrLinesLoading(true);
    void listInventoryGoodsReceiptLines(selectedGrId)
      .then((rows) => {
        if (!cancelled) setGrLines(rows);
      })
      .catch((err) => {
        if (!cancelled) setError(toErrorMessage(err));
      })
      .finally(() => {
        if (!cancelled) setGrLinesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedGrId]);

  useEffect(() => {
    if (!reqDetailOpen || !selectedReqId) {
      setReqEvents([]);
      return;
    }
    let cancelled = false;
    setReqEventsLoading(true);
    void listInventoryStateEvents("requisition", selectedReqId)
      .then((rows) => {
        if (!cancelled) setReqEvents(rows);
      })
      .catch(() => {
        if (!cancelled) setReqEvents([]);
      })
      .finally(() => {
        if (!cancelled) setReqEventsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [reqDetailOpen, selectedReqId, requisitions]);

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

  const runSaving = useCallback(
    async (work: () => Promise<void>) => {
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
    },
    [loadAll],
  );

  const openRequisitionDetails = useCallback((reqId: number) => {
    setSelectedReqId(reqId);
    setReqDetailOpen(true);
  }, []);

  /** Row-level requisition transition (list menus, independent of the open detail). */
  const runRequisitionTransition = useCallback(
    async (row: ProcurementRequisition, nextStatus: string, reason?: string) => {
      await runSaving(async () => {
        await transitionInventoryProcurementRequisition({
          requisition_id: row.id,
          expected_row_version: row.row_version,
          next_status: nextStatus,
          reason: reason ?? null,
        });
      });
    },
    [runSaving],
  );

  const runPurchaseOrderTransition = useCallback(
    async (row: PurchaseOrder, nextStatus: string) => {
      await runSaving(async () => {
        await transitionInventoryPurchaseOrder({
          purchase_order_id: row.id,
          expected_row_version: row.row_version,
          next_status: nextStatus,
        });
      });
    },
    [runSaving],
  );

  const transitionSelectedRequisition = async (nextStatus: string, reason?: string) => {
    if (!selectedRequisition) return;
    await runSaving(async () => {
      await transitionInventoryProcurementRequisition({
        requisition_id: selectedRequisition.id,
        expected_row_version: selectedRequisition.row_version,
        next_status: nextStatus,
        reason: reason ?? null,
      });
    });
  };

  const rejectSelectedRequisition = async () => {
    const reason = rejectReason.trim();
    if (!reason) return;
    await transitionSelectedRequisition("REJECTED", reason);
    setRejectOpen(false);
    setRejectReason("");
    setReqDetailOpen(false);
  };

  const openPoDetails = useCallback((poId: number) => {
    setSelectedPoId(poId);
    setPoDetailOpen(true);
  }, []);

  const openReceiveForPo = useCallback((poId: number) => {
    setReceiveInitialPoId(poId);
    setReceiveOpen(true);
  }, []);

  const openRepairableDetails = useCallback((orderId: number) => {
    setSelectedRepairableId(orderId);
    setRepairDetailOpen(true);
  }, []);

  const openGoodsReceipt = useCallback((receiptId: number) => {
    setActiveTab("goods-receipts");
    setSelectedGrId((current) => (current === receiptId ? null : receiptId));
  }, []);

  /** Direct repairable transition (no extra capture required). */
  const transitionRepairable = useCallback(
    async (order: RepairableOrder, nextStatus: string) => {
      await runSaving(async () => {
        await transitionInventoryRepairableOrder({
          order_id: order.id,
          expected_row_version: order.row_version,
          next_status: nextStatus,
        });
      });
    },
    [runSaving],
  );

  const confirmRepairAction = async (payload: RepairableActionPayload) => {
    if (!repairAction) return;
    const { kind, order } = repairAction;
    await runSaving(async () => {
      await transitionInventoryRepairableOrder({
        order_id: order.id,
        expected_row_version: order.row_version,
        next_status: REPAIRABLE_ACTION_TARGET[kind],
        reason: payload.reason ?? null,
        serial_number: payload.serial_number ?? null,
        vendor_supplier_id: payload.vendor_supplier_id ?? null,
        return_location_id: payload.return_location_id ?? null,
      });
    });
    setRepairAction(null);
  };

  /** Dispatch needs serial + vendor; reuse existing values when already captured. */
  const startRepairSend = useCallback(
    (order: RepairableOrder) => {
      if (order.serial_number && order.vendor_supplier_id) {
        void transitionRepairable(order, "SENT_FOR_REPAIR");
        return;
      }
      setRepairAction({ kind: "dispatch", order });
    },
    [transitionRepairable],
  );

  const startRepairReceive = useCallback(
    (order: RepairableOrder) => {
      if (order.return_location_id) {
        void transitionRepairable(order, "RETURNED_FROM_REPAIR");
        return;
      }
      setRepairAction({ kind: "receive", order });
    },
    [transitionRepairable],
  );

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
    { id: "goods-receipts", label: t("procurement.goodsReceipts.title") },
    { id: "repairables", label: t("procurement.repairables.title") },
    { id: "suppliers", label: t("procurement.suppliers.title") },
    { id: "lifecycle", label: "Last lifecycle events" },
  ];

  const purchaseOrderById = useMemo(() => {
    const map = new Map<number, PurchaseOrder>();
    for (const row of purchaseOrders) map.set(row.id, row);
    return map;
  }, [purchaseOrders]);

  /** Scorecard deliveries only carry the PO number — resolve it to an id for navigation. */
  const purchaseOrderIdByNumber = useMemo(() => {
    const map = new Map<string, number>();
    for (const row of purchaseOrders) map.set(row.po_number, row.id);
    return map;
  }, [purchaseOrders]);

  // ── Row action builders ─────────────────────────────────────────────────────
  const requisitionRowActions = useCallback(
    (row: ProcurementRequisition): ProcurementContextMenuAction[] => {
      const actions: ProcurementContextMenuAction[] = [
        {
          id: "open",
          label: t("procurement.requisitions.actions.open"),
          onClick: () => openRequisitionDetails(row.id),
        },
      ];
      if (row.status === "DRAFT") {
        actions.push({
          id: "submit",
          label: t("procurement.requisitions.actions.submit"),
          permission: P.INV_PROCURE,
          onClick: () => void runRequisitionTransition(row, "SUBMITTED"),
        });
      }
      if (row.status === "SUBMITTED") {
        actions.push({
          id: "approve",
          label: t("procurement.requisitions.actions.approve"),
          permission: P.INV_PROCURE,
          onClick: () => void runRequisitionTransition(row, "APPROVED"),
        });
        actions.push({
          id: "reject",
          label: t("procurement.requisitions.actions.reject"),
          permission: P.INV_PROCURE,
          destructive: true,
          onClick: () => {
            openRequisitionDetails(row.id);
            setRejectReason("");
            setRejectOpen(true);
          },
        });
      }
      const declineTarget = requisitionDeclineOrCloseTarget(row.status);
      if (declineTarget) {
        actions.push({
          id: "decline",
          label:
            declineTarget === "CLOSED"
              ? t("procurement.requisitions.actions.close")
              : t("procurement.requisitions.actions.cancel"),
          permission: P.INV_PROCURE,
          destructive: declineTarget === "CANCELLED",
          separatorBefore: true,
          onClick: () => void runRequisitionTransition(row, declineTarget),
        });
      }
      return actions;
    },
    [openRequisitionDetails, runRequisitionTransition, t],
  );

  const purchaseOrderRowActions = useCallback(
    (row: PurchaseOrder): ProcurementContextMenuAction[] => {
      const actions: ProcurementContextMenuAction[] = [
        {
          id: "open",
          label: t("procurement.purchaseOrders.actions.open"),
          onClick: () => openPoDetails(row.id),
        },
      ];
      if (row.status === "DRAFT") {
        actions.push({
          id: "submit",
          label: t("procurement.purchaseOrders.actions.submit"),
          permission: P.INV_PROCURE,
          onClick: () => void runPurchaseOrderTransition(row, "SUBMITTED"),
        });
      }
      if (row.status === "SUBMITTED") {
        actions.push({
          id: "approve",
          label: t("procurement.purchaseOrders.actions.approve"),
          permission: P.INV_PROCURE,
          onClick: () => void runPurchaseOrderTransition(row, "APPROVED"),
        });
      }
      if (isReceivablePurchaseOrder(row.status)) {
        actions.push({
          id: "receive",
          label: t("procurement.purchaseOrders.actions.receive"),
          permission: P.INV_PROCURE,
          onClick: () => openReceiveForPo(row.id),
        });
      }
      if (purchaseOrderCancelTarget(row.status)) {
        actions.push({
          id: "cancel",
          label: t("procurement.purchaseOrders.actions.cancel"),
          permission: P.INV_PROCURE,
          destructive: true,
          separatorBefore: true,
          onClick: () => void runPurchaseOrderTransition(row, "CANCELLED"),
        });
      }
      return actions;
    },
    [openPoDetails, openReceiveForPo, runPurchaseOrderTransition, t],
  );

  const repairableRowActions = useCallback(
    (row: RepairableOrder): ProcurementContextMenuAction[] => {
      const actions: ProcurementContextMenuAction[] = [
        {
          id: "open",
          label: t("procurement.repairables.actions.open"),
          onClick: () => openRepairableDetails(row.id),
        },
      ];
      if (row.status === "REQUESTED") {
        actions.push({
          id: "release",
          label: t("procurement.repairables.actions.release"),
          permission: P.INV_PROCURE,
          onClick: () => void transitionRepairable(row, "RELEASED"),
        });
      }
      if (row.status === "RELEASED") {
        actions.push({
          id: "send",
          label: t("procurement.repairables.actions.send"),
          permission: P.INV_PROCURE,
          onClick: () => startRepairSend(row),
        });
      }
      if (row.status === "SENT_FOR_REPAIR") {
        actions.push({
          id: "receive-back",
          label: t("procurement.repairables.actions.receiveBack"),
          permission: P.INV_PROCURE,
          onClick: () => startRepairReceive(row),
        });
      }
      if (row.status === "RETURNED_FROM_REPAIR") {
        // Closing requires repair cost / warranty capture — open the detail dialog.
        actions.push({
          id: "close",
          label: t("procurement.repairables.actions.close"),
          permission: P.INV_PROCURE,
          onClick: () => openRepairableDetails(row.id),
        });
      }
      if (row.status === "SENT_FOR_REPAIR" || row.status === "RETURNED_FROM_REPAIR") {
        actions.push({
          id: "scrap",
          label: t("procurement.repairables.actions.scrap"),
          permission: P.INV_PROCURE,
          destructive: true,
          separatorBefore: true,
          onClick: () => setRepairAction({ kind: "scrap", order: row }),
        });
      }
      if (row.status === "REQUESTED" || row.status === "RELEASED") {
        actions.push({
          id: "cancel",
          label: t("procurement.repairables.actions.cancel"),
          permission: P.INV_PROCURE,
          destructive: true,
          separatorBefore: true,
          onClick: () => setRepairAction({ kind: "cancel", order: row }),
        });
      }
      return actions;
    },
    [openRepairableDetails, startRepairReceive, startRepairSend, t, transitionRepairable],
  );

  // ── Column definitions ──────────────────────────────────────────────────────
  const reqColumns: ColumnDef<ProcurementRequisition>[] = useMemo(
    () => [
      { accessorKey: "req_number", header: t("procurement.requisitions.columns.number") },
      {
        accessorKey: "purchase_priority",
        header: t("procurement.requisition.priority"),
        cell: ({ row }) => formatOrDash(row.original.purchase_priority),
      },
      {
        accessorKey: "status",
        header: t("procurement.requisitions.columns.status"),
        cell: ({ row }) => (
          <Badge variant={lifecycleBadgeVariant(row.original.status)}>
            {t(`procurement.statuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      { accessorKey: "demand_source_type", header: t("procurement.requisitions.columns.source") },
      { accessorKey: "updated_at", header: t("procurement.requisitions.columns.updated") },
      {
        id: "actions",
        header: t("procurement.requisitions.columns.actions"),
        enableSorting: false,
        cell: ({ row }) => (
          <RowActionsMenu
            label={t("procurement.requisitions.actions.menu")}
            actions={requisitionRowActions(row.original)}
          />
        ),
      },
    ],
    [requisitionRowActions, t],
  );

  const poColumns: ColumnDef<PurchaseOrder>[] = useMemo(
    () => [
      { accessorKey: "po_number", header: t("procurement.purchaseOrders.columns.number") },
      {
        accessorKey: "status",
        header: t("procurement.purchaseOrders.columns.status"),
        cell: ({ row }) => (
          <Badge variant={lifecycleBadgeVariant(row.original.status)}>
            {t(`procurement.statuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        accessorKey: "supplier_company_name",
        header: t("procurement.purchaseOrders.columns.supplier"),
      },
      { accessorKey: "updated_at", header: t("procurement.purchaseOrders.columns.updated") },
      {
        id: "actions",
        header: t("procurement.purchaseOrders.columns.actions"),
        enableSorting: false,
        cell: ({ row }) => (
          <RowActionsMenu
            label={t("procurement.purchaseOrders.actions.menu")}
            actions={purchaseOrderRowActions(row.original)}
          />
        ),
      },
    ],
    [purchaseOrderRowActions, t],
  );

  const repairColumns: ColumnDef<RepairableOrder>[] = useMemo(
    () => [
      { accessorKey: "order_code", header: t("procurement.repairables.columns.order") },
      { accessorKey: "article_code", header: t("procurement.repairables.columns.article") },
      { accessorKey: "source_location_code", header: t("procurement.repairables.columns.source") },
      {
        id: "vendor",
        header: t("procurement.repairables.columns.vendor"),
        cell: ({ row }) => formatOrDash(row.original.vendor_supplier_name),
      },
      {
        accessorKey: "status",
        header: t("procurement.repairables.columns.status"),
        cell: ({ row }) => (
          <Badge variant={lifecycleBadgeVariant(row.original.status)}>
            {t(`procurement.repairableStatuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      { accessorKey: "updated_at", header: t("procurement.repairables.columns.updated") },
      {
        id: "actions",
        header: t("procurement.repairables.columns.actions"),
        enableSorting: false,
        cell: ({ row }) => (
          <RowActionsMenu
            label={t("procurement.repairables.actions.menu")}
            actions={repairableRowActions(row.original)}
          />
        ),
      },
    ],
    [repairableRowActions, t],
  );

  const grColumns: ColumnDef<GoodsReceipt>[] = useMemo(
    () => [
      { accessorKey: "gr_number", header: t("procurement.goodsReceipts.columns.number") },
      {
        id: "purchase_order",
        header: t("procurement.goodsReceipts.columns.po"),
        cell: ({ row }) => {
          const po = purchaseOrderById.get(row.original.purchase_order_id);
          if (!po) return "—";
          return (
            <Button
              variant="link"
              className="h-auto p-0"
              onClick={(event) => {
                event.stopPropagation();
                openPoDetails(po.id);
              }}
            >
              {po.po_number}
            </Button>
          );
        },
      },
      {
        accessorKey: "status",
        header: t("procurement.goodsReceipts.columns.status"),
        cell: ({ row }) => (
          <Badge variant={lifecycleBadgeVariant(row.original.status)}>
            {t(`procurement.statuses.${row.original.status}`, {
              defaultValue: row.original.status,
            })}
          </Badge>
        ),
      },
      {
        accessorKey: "posting_state",
        header: t("procurement.goodsReceipts.columns.posting"),
        cell: ({ row }) =>
          t(`procurement.postingStates.${row.original.posting_state}`, {
            defaultValue: row.original.posting_state,
          }),
      },
      {
        accessorKey: "received_at",
        header: t("procurement.goodsReceipts.columns.receivedAt"),
        cell: ({ row }) => formatOrDash(row.original.received_at),
      },
      {
        id: "actions",
        header: t("procurement.requisitions.columns.actions"),
        enableSorting: false,
        cell: ({ row }) => (
          <RowActionsMenu
            label={t("procurement.goodsReceipts.actions.menu")}
            actions={[
              {
                id: "open-po",
                label: t("procurement.goodsReceipts.actions.openPo"),
                onClick: () => openPoDetails(row.original.purchase_order_id),
              },
              {
                id: "view-lines",
                label: t("procurement.goodsReceipts.actions.viewLines"),
                onClick: () => openGoodsReceipt(row.original.id),
              },
            ]}
          />
        ),
      },
    ],
    [openGoodsReceipt, openPoDetails, purchaseOrderById, t],
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

  const filteredRequisitions = useMemo(() => {
    const term = reqSearch.trim().toLowerCase();
    return requisitions.filter((row) => {
      if (reqStatusFilter !== ALL && row.status !== reqStatusFilter) return false;
      return includesTerm(term, row.req_number, row.demand_source_ref, row.demand_source_type);
    });
  }, [requisitions, reqSearch, reqStatusFilter]);

  const filteredPurchaseOrders = useMemo(() => {
    const term = poSearch.trim().toLowerCase();
    return purchaseOrders.filter((row) => {
      if (poStatusFilter !== ALL && row.status !== poStatusFilter) return false;
      if (poSupplierFilter !== ALL && String(row.supplier_id) !== poSupplierFilter) return false;
      return includesTerm(term, row.po_number, row.supplier_company_name, row.supplier_name);
    });
  }, [purchaseOrders, poSearch, poStatusFilter, poSupplierFilter]);

  const filteredRepairables = useMemo(() => {
    const term = repairSearch.trim().toLowerCase();
    return repairables.filter((row) => {
      if (repairStatusFilter !== ALL && row.status !== repairStatusFilter) return false;
      if (
        repairVendorFilter !== ALL &&
        String(row.vendor_supplier_id ?? 0) !== repairVendorFilter
      ) {
        return false;
      }
      return includesTerm(
        term,
        row.order_code,
        row.article_code,
        row.article_name,
        row.serial_number,
      );
    });
  }, [repairables, repairSearch, repairStatusFilter, repairVendorFilter]);

  const filteredInventorySuppliers = useMemo(() => {
    const term = supplierSearch.trim().toLowerCase();
    return inventorySuppliers.filter((s) => {
      if (supplierStatusFilter !== "__all__" && s.status_code !== supplierStatusFilter)
        return false;
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
  }, [
    stateEvents,
    eventFromDate,
    eventToDate,
    eventArticleFilter,
    eventWarehouseFilter,
    repairById,
  ]);

  // ── Non-blocked suppliers for PO creation ───────────────────────────────────
  const nonBlockedSuppliers = useMemo(
    () => inventorySuppliers.filter((s) => s.status_code !== "BLOCKED"),
    [inventorySuppliers],
  );

  /** Active, non-blocked suppliers eligible as repair vendors. */
  const repairVendorOptions = useMemo(
    () => nonBlockedSuppliers.filter((s) => s.is_active === 1),
    [nonBlockedSuppliers],
  );

  const reqFilterDefs: SmartFilterDef[] = useMemo(
    () => [
      {
        id: "req-status",
        kind: "select",
        label: t("procurement.requisitions.filters.status"),
        value: reqStatusFilter,
        options: REQ_KANBAN_STATUSES.map((status) => ({
          value: status,
          label: t(`procurement.statuses.${status}`, { defaultValue: status }),
        })),
        onChange: (next) => setReqStatusFilter(next ?? ALL),
      },
    ],
    [reqStatusFilter, t],
  );

  const poFilterDefs: SmartFilterDef[] = useMemo(
    () => [
      {
        id: "po-status",
        kind: "select",
        label: t("procurement.purchaseOrders.filters.status"),
        value: poStatusFilter,
        options: PO_KANBAN_STATUSES.map((status) => ({
          value: status,
          label: t(`procurement.statuses.${status}`, { defaultValue: status }),
        })),
        onChange: (next) => setPoStatusFilter(next ?? ALL),
      },
      {
        id: "po-supplier",
        kind: "select",
        label: t("procurement.purchaseOrders.filters.supplier"),
        value: poSupplierFilter,
        options: inventorySuppliers.map((supplier) => ({
          value: String(supplier.id),
          label: `${supplier.code} — ${supplier.name}`,
        })),
        onChange: (next) => setPoSupplierFilter(next ?? ALL),
      },
    ],
    [inventorySuppliers, poStatusFilter, poSupplierFilter, t],
  );

  const repairFilterDefs: SmartFilterDef[] = useMemo(
    () => [
      {
        id: "repair-status",
        kind: "select",
        label: t("procurement.repairables.filters.status"),
        value: repairStatusFilter,
        options: REPAIRABLE_STATUSES.map((status) => ({
          value: status,
          label: t(`procurement.repairableStatuses.${status}`, { defaultValue: status }),
        })),
        onChange: (next) => setRepairStatusFilter(next ?? ALL),
      },
      {
        id: "repair-vendor",
        kind: "select",
        label: t("procurement.repairables.filters.vendor"),
        value: repairVendorFilter,
        options: inventorySuppliers.map((supplier) => ({
          value: String(supplier.id),
          label: `${supplier.code} — ${supplier.name}`,
        })),
        onChange: (next) => setRepairVendorFilter(next ?? ALL),
      },
    ],
    [inventorySuppliers, repairStatusFilter, repairVendorFilter, t],
  );

  const resetReqFilters = () => {
    setReqSearchInput("");
    setReqSearch("");
    setReqStatusFilter(ALL);
  };

  const resetPoFilters = () => {
    setPoSearchInput("");
    setPoSearch("");
    setPoStatusFilter(ALL);
    setPoSupplierFilter(ALL);
  };

  const resetRepairFilters = () => {
    setRepairSearchInput("");
    setRepairSearch("");
    setRepairStatusFilter(ALL);
    setRepairVendorFilter(ALL);
  };

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
                <SmartFilterBar
                  className="min-w-0 flex-1"
                  searchPlaceholder={t("procurement.requisitions.searchPlaceholder")}
                  searchValue={reqSearchInput}
                  onSearchInputChange={setReqSearchInput}
                  onSearchChange={setReqSearch}
                  filters={reqFilterDefs}
                  resultCount={filteredRequisitions.length}
                  onReset={resetReqFilters}
                />
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
                              {t("procurement.requisitions.kanbanEmpty")}
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
                  {!loading && requisitions.length === 0 ? (
                    <EmptyState
                      title={t("procurement.requisitions.empty")}
                      description={t("procurement.requisitions.emptyDescription")}
                      {...(canProcure
                        ? {
                            actionLabel: t("procurement.recommendations.createRequisition"),
                            onAction: () => setCreateReqOpen(true),
                          }
                        : {})}
                    />
                  ) : (
                    <DataTable
                      columns={reqColumns}
                      data={filteredRequisitions}
                      isLoading={loading}
                      searchable={false}
                      onRowClick={(req) => openRequisitionDetails(req.id)}
                    />
                  )}
                </div>
              ) : null}
            </div>
          ) : null}

          {/* ── PURCHASE ORDERS ── */}
          {activeTab === "purchase-orders" ? (
            <div className="rounded-md border p-4">
              <h3 className="mb-3 text-sm font-semibold">Purchase orders</h3>
              <div className="mb-3">
                <SmartFilterBar
                  searchPlaceholder={t("procurement.purchaseOrders.searchPlaceholder")}
                  searchValue={poSearchInput}
                  onSearchInputChange={setPoSearchInput}
                  onSearchChange={setPoSearch}
                  filters={poFilterDefs}
                  resultCount={filteredPurchaseOrders.length}
                  onReset={resetPoFilters}
                />
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
                              {t("procurement.purchaseOrders.kanbanEmpty")}
                            </div>
                          ) : null}
                        </div>
                      </div>
                    );
                  })}
                </div>
              ) : !loading && purchaseOrders.length === 0 ? (
                <EmptyState
                  title={t("procurement.purchaseOrders.empty")}
                  description={t("procurement.purchaseOrders.emptyDescription")}
                  {...(canProcure
                    ? {
                        actionLabel: t("procurement.purchaseOrders.create"),
                        onAction: () => {
                          setPoDialogReqId(approvedRequisitions[0]?.id ?? 0);
                          setCreatePoOpen(true);
                        },
                      }
                    : {})}
                />
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
              <h3 className="mb-2 text-sm font-semibold">{t("procurement.repairables.title")}</h3>
              <div className="mb-3">
                <SmartFilterBar
                  searchPlaceholder={t("procurement.repairables.searchPlaceholder")}
                  searchValue={repairSearchInput}
                  onSearchInputChange={setRepairSearchInput}
                  onSearchChange={setRepairSearch}
                  filters={repairFilterDefs}
                  resultCount={filteredRepairables.length}
                  onReset={resetRepairFilters}
                />
              </div>
              {!loading && repairables.length === 0 ? (
                <EmptyState
                  title={t("procurement.repairables.empty")}
                  description={t("procurement.repairables.emptyDescription")}
                  {...(canProcure
                    ? {
                        actionLabel: t("procurement.repairables.create"),
                        onAction: () => setRepairOpen(true),
                      }
                    : {})}
                />
              ) : (
                <DataTable
                  columns={repairColumns}
                  data={filteredRepairables}
                  isLoading={loading}
                  searchable={false}
                  onRowClick={(order) => openRepairableDetails(order.id)}
                />
              )}
            </div>
          ) : null}

          {/* ── GOODS RECEIPTS ── */}
          {activeTab === "goods-receipts" ? (
            <div className="rounded-md border p-4">
              <h3 className="mb-3 text-sm font-semibold">{t("procurement.goodsReceipts.title")}</h3>
              {!loading && goodsReceipts.length === 0 ? (
                <EmptyState
                  title={t("procurement.goodsReceipts.empty")}
                  description={t("procurement.goodsReceipts.emptyDescription")}
                  {...(canProcure
                    ? {
                        actionLabel: t("procurement.receive.title"),
                        onAction: () => {
                          setReceiveInitialPoId(null);
                          setReceiveOpen(true);
                        },
                      }
                    : {})}
                />
              ) : (
                <DataTable
                  columns={grColumns}
                  data={goodsReceipts}
                  isLoading={loading}
                  searchable={false}
                  onRowClick={(receipt) => openGoodsReceipt(receipt.id)}
                />
              )}
              {selectedGrId ? (
                <div className="mt-4 rounded-md border p-3">
                  <h4 className="mb-2 text-xs font-semibold uppercase text-text-muted">
                    {t("procurement.goodsReceipts.lines.title")}
                  </h4>
                  {grLinesLoading ? (
                    <p className="text-sm text-text-muted">
                      {t("procurement.goodsReceipts.lines.loading")}
                    </p>
                  ) : grLines.length === 0 ? (
                    <p className="text-sm text-text-muted">
                      {t("procurement.goodsReceipts.lines.empty")}
                    </p>
                  ) : (
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead>
                            {t("procurement.goodsReceipts.lines.columns.article")}
                          </TableHead>
                          <TableHead>
                            {t("procurement.goodsReceipts.lines.columns.location")}
                          </TableHead>
                          <TableHead className="text-right">
                            {t("procurement.goodsReceipts.lines.columns.received")}
                          </TableHead>
                          <TableHead className="text-right">
                            {t("procurement.goodsReceipts.lines.columns.accepted")}
                          </TableHead>
                          <TableHead className="text-right">
                            {t("procurement.goodsReceipts.lines.columns.rejected")}
                          </TableHead>
                          <TableHead>
                            {t("procurement.goodsReceipts.lines.columns.reason")}
                          </TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {grLines.map((line) => (
                          <TableRow key={line.id}>
                            <TableCell>
                              {formatAssetLabel(line.article_code, line.article_name)}
                            </TableCell>
                            <TableCell>{line.location_code}</TableCell>
                            <TableCell className="text-right tabular-nums">
                              {line.received_qty}
                            </TableCell>
                            <TableCell className="text-right tabular-nums">
                              {line.accepted_qty}
                            </TableCell>
                            <TableCell className="text-right tabular-nums">
                              {line.rejected_qty}
                            </TableCell>
                            <TableCell>{formatOrDash(line.rejection_reason)}</TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  )}
                </div>
              ) : null}
            </div>
          ) : null}

          {/* ── SUPPLIERS ── */}
          {activeTab === "suppliers" ? (
            <div className="rounded-md border p-4">
              <div className="mb-3 flex items-center justify-between">
                <h3 className="text-sm font-semibold">{t("procurement.suppliers.title")}</h3>
                <PermissionGate permission={P.INV_MANAGE}>
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
                      <PermissionGate permission={P.INV_MANAGE}>
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
                      <div className="rounded-md border p-3 text-sm text-text-muted">Loading…</div>
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
                                  ? new Date(
                                      supplierScorecard.last_purchase_at,
                                    ).toLocaleDateString()
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
                                    {t(
                                      "procurement.suppliers.scorecard.deliveryColumns.receivedAt",
                                    )}
                                  </TableHead>
                                  <TableHead className="h-7 px-2 text-xs">
                                    {t("procurement.suppliers.scorecard.deliveryColumns.po")}
                                  </TableHead>
                                  <TableHead className="h-7 px-2 text-xs">
                                    {t("procurement.suppliers.scorecard.deliveryColumns.article")}
                                  </TableHead>
                                  <TableHead className="h-7 px-2 text-right text-xs">
                                    {t(
                                      "procurement.suppliers.scorecard.deliveryColumns.orderedQty",
                                    )}
                                  </TableHead>
                                  <TableHead className="h-7 px-2 text-right text-xs">
                                    {t(
                                      "procurement.suppliers.scorecard.deliveryColumns.acceptedQty",
                                    )}
                                  </TableHead>
                                  <TableHead className="h-7 px-2 text-right text-xs">
                                    {t(
                                      "procurement.suppliers.scorecard.deliveryColumns.rejectedQty",
                                    )}
                                  </TableHead>
                                  <TableHead className="h-7 px-2 text-right text-xs">
                                    {t("procurement.suppliers.scorecard.deliveryColumns.leadTime")}
                                  </TableHead>
                                </TableRow>
                              </TableHeader>
                              <TableBody>
                                {supplierScorecard.last_deliveries.map((delivery, index) => (
                                  <TableRow
                                    key={`${delivery.po_number}-${delivery.article_code}-${index}`}
                                  >
                                    <TableCell className="px-2 py-1.5 text-xs">
                                      {delivery.received_at
                                        ? new Date(delivery.received_at).toLocaleDateString()
                                        : "—"}
                                    </TableCell>
                                    <TableCell className="px-2 py-1.5 font-mono text-xs">
                                      {(() => {
                                        const poId = purchaseOrderIdByNumber.get(
                                          delivery.po_number,
                                        );
                                        if (!poId) return formatEntityCode(delivery.po_number);
                                        return (
                                          <button
                                            type="button"
                                            className="font-mono font-medium text-primary hover:underline"
                                            onClick={() => openPoDetails(poId)}
                                          >
                                            {formatEntityCode(delivery.po_number)}
                                          </button>
                                        );
                                      })()}
                                    </TableCell>
                                    <TableCell className="px-2 py-1.5 text-xs">
                                      {formatAssetLabel(
                                        delivery.article_code,
                                        delivery.article_name,
                                      )}
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
                        <PermissionGate permission={P.INV_MANAGE}>
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
                                <span className="text-text-muted">min {price.min_order_qty}</span>
                              ) : null}
                              {price.valid_from ? (
                                <span className="text-text-muted">
                                  from {new Date(price.valid_from).toLocaleDateString()}
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
                        <PermissionGate permission={P.INV_MANAGE}>
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
            <DialogTitle className="flex flex-wrap items-center gap-2">
              {selectedRequisition ? (
                <>
                  <span>{selectedRequisition.req_number}</span>
                  <Badge variant={lifecycleBadgeVariant(selectedRequisition.status)}>
                    {t(`procurement.statuses.${selectedRequisition.status}`, {
                      defaultValue: selectedRequisition.status,
                    })}
                  </Badge>
                </>
              ) : (
                "Request details"
              )}
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
              <div className="rounded-md border p-3 text-sm">
                <div className="mb-2 font-medium">
                  {t("procurement.requisitions.history.title")}
                </div>
                {reqEventsLoading ? (
                  <div className="text-text-muted">
                    {t("procurement.requisitions.history.loading")}
                  </div>
                ) : reqEvents.length === 0 ? (
                  <div className="text-text-muted">
                    {t("procurement.requisitions.history.empty")}
                  </div>
                ) : (
                  <ul className="space-y-2">
                    {reqEvents.map((event) => (
                      <li key={event.id} className="border-b pb-2 last:border-0 last:pb-0">
                        <div className="flex flex-wrap items-center gap-1 text-xs">
                          <span className="font-medium">
                            {event.from_status
                              ? t(`procurement.statuses.${event.from_status}`, {
                                  defaultValue: event.from_status,
                                })
                              : "—"}
                          </span>
                          <span aria-hidden>→</span>
                          <span className="font-medium">
                            {t(`procurement.statuses.${event.to_status}`, {
                              defaultValue: event.to_status,
                            })}
                          </span>
                          <span className="ml-auto text-text-muted">{event.changed_at}</span>
                        </div>
                        {event.reason ? (
                          <div className="mt-0.5 text-xs text-text-muted">{event.reason}</div>
                        ) : null}
                      </li>
                    ))}
                  </ul>
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
              <PermissionGate permission={P.INV_PROCURE}>
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
                  {selectedRequisition.status === "SUBMITTED" ? (
                    <Button
                      variant="destructive"
                      disabled={saving}
                      onClick={() => {
                        setRejectReason("");
                        setRejectOpen(true);
                      }}
                    >
                      {t("procurement.requisitions.actions.reject")}
                    </Button>
                  ) : null}
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

      {/* Requisition rejection dialog — reason is mandatory and terminal. */}
      <Dialog open={rejectOpen} onOpenChange={setRejectOpen}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("procurement.requisitions.reject.title")}</DialogTitle>
          </DialogHeader>
          <p className="text-xs text-text-muted">{t("procurement.requisitions.reject.hint")}</p>
          <div className="space-y-1">
            <Label htmlFor="inv-req-reject-reason">
              {t("procurement.requisitions.reject.reason")}
            </Label>
            <Textarea
              id="inv-req-reject-reason"
              rows={3}
              value={rejectReason}
              onChange={(event) => setRejectReason(event.target.value)}
              placeholder={t("procurement.requisitions.reject.reasonPlaceholder")}
            />
          </div>
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="outline" onClick={() => setRejectOpen(false)}>
              {t("procurement.receive.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={saving || !rejectReason.trim()}
              onClick={() => void rejectSelectedRequisition()}
            >
              {t("procurement.requisitions.reject.confirm")}
            </Button>
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
        reloadToken={poReloadToken}
        onReceiveGoods={openReceiveForPo}
        onOpenRequisition={(requisitionId) => {
          setPoDetailOpen(false);
          setActiveTab("requisitions");
          openRequisitionDetails(requisitionId);
        }}
        onOpenGoodsReceipt={(receiptId) => {
          setPoDetailOpen(false);
          openGoodsReceipt(receiptId);
        }}
      />

      <RepairableDetailDialog
        open={repairDetailOpen}
        onOpenChange={setRepairDetailOpen}
        orderId={selectedRepairableId}
        suppliers={repairVendorOptions}
        locations={locations}
        {...(onOpenArticle ? { onOpenArticle } : {})}
        onTransitioned={() => {
          void loadAll();
        }}
      />

      {/* Repairable lifecycle capture (dispatch / scrap / cancel / receive back) */}
      <RepairableActionDialog
        open={repairAction !== null}
        onOpenChange={(next) => {
          if (!next) setRepairAction(null);
        }}
        kind={repairAction?.kind ?? "scrap"}
        order={repairAction?.order ?? null}
        suppliers={repairVendorOptions}
        locations={locations}
        saving={saving}
        onConfirm={confirmRepairAction}
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
                    source_reorder_trigger:
                      reqSourceType === "REORDER" ? "threshold_crossed" : null,
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
        <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
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

      {/* Goods receipt dialog — posts every line in a single call */}
      <ReceiveGoodsDialog
        open={receiveOpen}
        onOpenChange={setReceiveOpen}
        purchaseOrders={purchaseOrders}
        locations={locations}
        initialPoId={receiveInitialPoId}
        onPosted={() => {
          setPoReloadToken((token) => token + 1);
          void loadAll();
        }}
      />

      {/* Repairable order dialog */}
      <Dialog open={repairOpen} onOpenChange={setRepairOpen}>
        <DialogContent
          className="max-h-[90vh] max-w-2xl overflow-y-auto"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle>{t("procurement.repairables.form.title")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-2 md:grid-cols-2">
            <div className="space-y-1">
              <Label>{t("procurement.repairables.form.article")}</Label>
              <Select
                value={String(repairArticleId)}
                onValueChange={(v) => setRepairArticleId(Number(v))}
              >
                <SelectTrigger>
                  <SelectValue placeholder={t("procurement.repairables.form.article")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.repairables.form.selectArticle")}
                  </SelectItem>
                  {articles.map((a) => (
                    <SelectItem key={a.id} value={String(a.id)}>
                      {formatAssetLabel(a.article_code, a.article_name)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("procurement.repairables.form.sourceLocation")}</Label>
              <Select
                value={String(repairSourceLocationId)}
                onValueChange={(v) => setRepairSourceLocationId(Number(v))}
              >
                <SelectTrigger>
                  <SelectValue placeholder={t("procurement.repairables.form.sourceLocation")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.repairables.form.selectSource")}
                  </SelectItem>
                  {locations.map((l) => (
                    <SelectItem key={l.id} value={String(l.id)}>
                      {l.warehouse_code}/{l.code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("procurement.repairables.form.returnLocation")}</Label>
              <Select
                value={String(repairReturnLocationId)}
                onValueChange={(v) => setRepairReturnLocationId(Number(v))}
              >
                <SelectTrigger>
                  <SelectValue placeholder={t("procurement.repairables.form.returnLocation")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.repairables.form.noReturnLocation")}
                  </SelectItem>
                  {locations.map((l) => (
                    <SelectItem key={l.id} value={String(l.id)}>
                      {l.warehouse_code}/{l.code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-repair-qty">{t("procurement.repairables.form.quantity")}</Label>
              <Input
                id="inv-repair-qty"
                type="number"
                min={0}
                step="0.01"
                value={repairQty}
                onChange={(e) => setRepairQty(Number(e.target.value || 0))}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-repair-serial">{t("procurement.repairables.form.serial")}</Label>
              <Input
                id="inv-repair-serial"
                value={repairSerial}
                onChange={(e) => setRepairSerial(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label>{t("procurement.repairables.form.vendor")}</Label>
              <Select
                value={String(repairVendorId)}
                onValueChange={(v) => setRepairVendorId(Number(v))}
              >
                <SelectTrigger>
                  <SelectValue placeholder={t("procurement.repairables.form.vendor")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("procurement.repairables.form.noVendor")}</SelectItem>
                  {repairVendorOptions.map((supplier) => (
                    <SelectItem key={supplier.id} value={String(supplier.id)}>
                      {supplier.code} — {supplier.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1 md:col-span-2">
              <Label htmlFor="inv-repair-reason">{t("procurement.repairables.form.reason")}</Label>
              <Input
                id="inv-repair-reason"
                value={repairReason}
                onChange={(e) => setRepairReason(e.target.value)}
              />
            </div>
          </div>
          {repairArticleId > 0 ? (
            <div className="mt-3 rounded-md border border-dashed p-2 text-xs">
              <div className="mb-1 font-semibold">
                {t("procurement.repairables.form.balancesTitle")}
              </div>
              {repairBalances.length === 0 ? (
                <div className="text-muted-foreground">
                  {t("procurement.repairables.form.balancesEmpty")}
                </div>
              ) : (
                <ul className="space-y-1">
                  {repairBalances.map((b) => (
                    <li key={`${b.id}-${b.location_id}`}>
                      {b.warehouse_code}/{b.location_code}:{" "}
                      {t("procurement.repairables.form.balanceRow", {
                        onHand: b.on_hand_qty,
                        reserved: b.reserved_qty,
                        available: b.available_qty,
                      })}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : null}
          <DialogFooter className="gap-2 sm:gap-0">
            <Button type="button" variant="outline" onClick={() => setRepairOpen(false)}>
              {t("procurement.repairables.form.cancel")}
            </Button>
            <Button
              type="button"
              disabled={
                saving || repairArticleId <= 0 || repairSourceLocationId <= 0 || repairQty <= 0
              }
              onClick={() =>
                void runSaving(async () => {
                  await createInventoryRepairableOrder({
                    article_id: repairArticleId,
                    quantity: repairQty,
                    source_location_id: repairSourceLocationId,
                    return_location_id: repairReturnLocationId > 0 ? repairReturnLocationId : null,
                    reason: repairReason.trim() || null,
                    serial_number: repairSerial.trim() || null,
                    vendor_supplier_id: repairVendorId > 0 ? repairVendorId : null,
                  });
                  setRepairSerial("");
                  setRepairVendorId(0);
                  setRepairOpen(false);
                })
              }
            >
              {t("procurement.repairables.form.submit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Supplier create/edit dialog */}
      <Dialog open={supplierEditOpen} onOpenChange={setSupplierEditOpen}>
        <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>
              {supplierForm.id ? t("procurement.suppliers.edit") : t("procurement.suppliers.new")}
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
                onChange={(e) => setSupplierForm((s) => ({ ...s, isActive: e.target.checked }))}
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
