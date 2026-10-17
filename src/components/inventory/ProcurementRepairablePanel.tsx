import type { ColumnDef } from "@tanstack/react-table";
import { Columns3, List, MoreHorizontal } from "lucide-react";
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { ProcurementAlertCards } from "@/components/inventory/ProcurementAlertCards";
import { ProcurementArchivePanel } from "@/components/inventory/ProcurementArchivePanel";
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
import {
  type RequisitionDraftLine,
  RequisitionLinesEditor,
  draftLinesToInput,
  emptyRequisitionLine,
  requisitionLinesBlocker,
} from "@/components/inventory/RequisitionLinesEditor";
import { SupplierArticleSourcesSection } from "@/components/inventory/SupplierArticleSourcesSection";
import { SupplierContactsSection } from "@/components/inventory/SupplierContactsSection";
import { riskBadgeVariant } from "@/components/inventory/supplier-sourcing";
import { useLookupOptions } from "@/components/inventory/use-lookup-options";
import { KanbanBoard } from "@/components/kanban";
import type { KanbanCard, KanbanColumn } from "@/components/kanban";
import { type ViewOption, ViewToggle } from "@/components/layout/ModuleHeaderControls";
import { KpiTile } from "@/components/metrics/KpiTile";
import { Timeline } from "@/components/timeline";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
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
import { StatusChip } from "@/components/ui/status-chip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Textarea } from "@/components/ui/textarea";
import {
  POSTING_STATE_TONE,
  PROCUREMENT_STATUS_TONE,
  PURCHASE_PRIORITY_TONE,
  REPAIRABLE_STATUS_TONE,
  SUPPLIER_STATUS_TONE,
  toneOf,
} from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { formatAssetLabel, formatEntityCode, formatOrDash } from "@/lib/display";
import { cn } from "@/lib/utils";
import {
  type InventoryArticleRow,
  type InventorySupplierRow,
  type ProcurementRequisitionLineRow,
  type ProcurementRequisitionRow,
  type PurchaseOrderRow,
  createInventoryMultiLineRequisition,
  createInventoryPurchaseOrderFromRequisition,
  createInventoryRepairableOrder,
  deactivateInventorySupplier,
  getInventorySupplierScorecard,
  getProcurementAlerts,
  getProcurementDashboardSummary,
  listCurrencyExchangeRates,
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
  replaceInventoryRequisitionLines,
  transitionInventoryProcurementRequisition,
  transitionInventoryPurchaseOrder,
  transitionInventoryRepairableOrder,
  upsertInventoryDocumentLink,
  upsertInventorySupplier,
  upsertSupplierPrice,
} from "@/services/inventory-service";
import type {
  GoodsReceipt,
  GoodsReceiptLine,
  InventoryDocumentLink,
  InventoryStateEvent,
  InventoryStockBalance,
  ProcurementAlert,
  ProcurementDashboardSummary,
  ProcurementRequisition,
  PurchaseOrder,
  RepairableOrder,
  StockLocation,
  SupplierPrice,
  SupplierPurchaseHistoryRow,
  SupplierScorecard,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Requisition form values set from another surface (article detail, recommendations). */
export type RequisitionPrefill = {
  articleId: number;
  locationId?: number | null;
  quantity?: number;
  sourceType?: string;
  sourceRef?: string | null;
  reason?: string | null;
};

/** One-shot request from the inventory page, handled once the panel is mounted. */
export type ProcurementPanelRequest =
  | { kind: "create-requisition"; articleId: number; locationId: number | null }
  | { kind: "open-purchase-order"; purchaseOrderId: number };

export type ProcurementRepairablePanelHandle = {
  openCreateRequisition: (prefill?: RequisitionPrefill) => void;
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
  /** Action requested by the page (e.g. "create a requisition for this article"). */
  request?: ProcurementPanelRequest | null;
  /** Called once `request` has been acted on, so it does not replay on remount. */
  onRequestHandled?: () => void;
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

/**
 * Demand sources a user can pick for a manual requisition. Work-order, repairable, count and PM
 * requisitions are raised from those records so they keep their link.
 */
const MANUAL_DEMAND_SOURCES = ["REORDER", "MANUAL"] as const;

const ALL = "__all__";
/** Status-set filters a dashboard tile opens (statuses sent by the backend with its counts). */
const OPEN = "__open__";
const LATE = "__late__";
/** Element the low-stock tiles bring into view (the evaluate_reorder recommendations). */
const RECOMMENDATIONS_ID = "procurement-recommendations";

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

/**
 * Board moves = the one-click lifecycle steps the row menu already offers (submit, approve),
 * run through the same `transition_*` command and server guards. Reject (reason), cancel and
 * close (confirmation) stay dialog-only and are never offered as a drop target.
 */
const BOARD_ONE_CLICK_MOVES: Record<string, string> = {
  DRAFT: "SUBMITTED",
  SUBMITTED: "APPROVED",
};

function isBoardOneClickMove(from: string, to: string): boolean {
  return BOARD_ONE_CLICK_MOVES[from] === to;
}

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

/** Destructive / irreversible step waiting for the user's confirmation. */
type PendingConfirm = {
  title: string;
  description: string;
  confirmLabel: string;
  run: () => Promise<boolean>;
};

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
        className="h-10 w-10"
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
  const {
    viewMode: viewModeProp,
    onViewModeChange,
    onOpenArticle,
    request,
    onRequestHandled,
  } = props;
  const { t } = useTranslation(["inventory", "common"]);
  const { formatDate, formatDateTime, formatNumber, formatCurrency, formatPercent, currencyCode } =
    useFormatters();
  const { can } = usePermissions();
  const canProcure = can(P.INV_PROCURE);
  const demandSources = useLookupOptions("inventory.demand_source_type");
  const purchasePriorities = useLookupOptions("inventory.purchase_priority");
  const documentPurposes = useLookupOptions("inventory.document_link_purpose");

  const qtyLabel = useCallback(
    (v: number | null | undefined) =>
      v == null ? "—" : formatNumber(v, { maximumFractionDigits: 3 }),
    [formatNumber],
  );
  const moneyLabel = useCallback(
    (v: number | null | undefined) => (v == null ? "—" : formatCurrency(v)),
    [formatCurrency],
  );
  const pctLabel = useCallback(
    (v: number | null | undefined) => (v == null ? "—" : formatPercent(v)),
    [formatPercent],
  );
  const dateLabel = useCallback(
    (v: string | null | undefined) => (v ? formatDate(v) : "—"),
    [formatDate],
  );
  const sourceLabel = useCallback(
    (code: string | null | undefined) =>
      code
        ? t(`procurement.requisition.sources.${code}`, {
            defaultValue: demandSources.labelOf(code),
          })
        : "—",
    [demandSources, t],
  );
  const priorityLabel = useCallback(
    (code: string | null | undefined) =>
      code
        ? t(`procurement.requisition.priorityValues.${code}`, {
            defaultValue: purchasePriorities.labelOf(code),
          })
        : "—",
    [purchasePriorities, t],
  );

  // ── Core state ─────────────────────────────────────────────────────────────
  const [activeTab, setActiveTab] = useState<SubTab>("requisitions");
  const [articles, setArticles] = useState<InventoryArticleRow[]>([]);
  const [locations, setLocations] = useState<StockLocation[]>([]);
  const [inventorySuppliers, setInventorySuppliers] = useState<InventorySupplierRow[]>([]);
  const [requisitions, setRequisitions] = useState<ProcurementRequisitionRow[]>([]);
  const [selectedReqId, setSelectedReqId] = useState<number | null>(null);
  const [reqLines, setReqLines] = useState<ProcurementRequisitionLineRow[]>([]);
  const [reqLinesError, setReqLinesError] = useState<unknown>(null);
  /** Bumped after the lines of a draft are replaced so the detail refetches them. */
  const [reqLinesVersion, setReqLinesVersion] = useState(0);
  const [purchaseOrders, setPurchaseOrders] = useState<PurchaseOrderRow[]>([]);
  const [selectedPoId, setSelectedPoId] = useState<number | null>(null);
  const [repairables, setRepairables] = useState<RepairableOrder[]>([]);
  const [goodsReceipts, setGoodsReceipts] = useState<GoodsReceipt[]>([]);
  const [selectedGrId, setSelectedGrId] = useState<number | null>(null);
  const [grLines, setGrLines] = useState<GoodsReceiptLine[]>([]);
  const [grLinesLoading, setGrLinesLoading] = useState(false);
  const [grLinesError, setGrLinesError] = useState<unknown>(null);
  const [stateEvents, setStateEvents] = useState<InventoryStateEvent[]>([]);
  const [dashboard, setDashboard] = useState<ProcurementDashboardSummary | null>(null);
  const [alerts, setAlerts] = useState<ProcurementAlert[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  /** Failure of the last full load: lists are replaced by an ErrorState with retry. */
  const [loadError, setLoadError] = useState<unknown>(null);
  const [pendingConfirm, setPendingConfirm] = useState<PendingConfirm | null>(null);

  // ── Requisition form ────────────────────────────────────────────────────────
  // Manual requisitions are stock replenishment or a free demand. Requisitions for a work
  // order part are raised from the work order (shortage panel), which carries the WO link.
  const [reqDraftLines, setReqDraftLines] = useState<RequisitionDraftLine[]>(() => [
    emptyRequisitionLine(),
  ]);
  /** Draft requisition whose lines are being edited; null when creating a requisition. */
  const [reqEditTarget, setReqEditTarget] = useState<ProcurementRequisitionRow | null>(null);
  const [reqSourceType, setReqSourceType] = useState("REORDER");
  const [reqSourceRef, setReqSourceRef] = useState<string>("");
  const [reqReason, setReqReason] = useState<string>("");
  const [reqPriority, setReqPriority] = useState<string>("NORMAL");

  // ── PO form ─────────────────────────────────────────────────────────────────
  const [poSupplierId, setPoSupplierId] = useState<number>(0);
  const [poDialogReqId, setPoDialogReqId] = useState<number>(0);
  /** Currency the PO is priced in; follows the supplier's invoicing currency by default. */
  const [poCurrency, setPoCurrency] = useState<string>("");
  /** Currencies with a stored exchange rate (offered for foreign-currency POs). */
  const [rateCurrencies, setRateCurrencies] = useState<string[]>([]);

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
  const [supplierDetailError, setSupplierDetailError] = useState<unknown>(null);
  const [supplierDetailVersion, setSupplierDetailVersion] = useState(0);
  const [supplierEditOpen, setSupplierEditOpen] = useState(false);
  const [supplierForm, setSupplierForm] = useState<{
    id: number | null;
    rowVersion: number | null;
    code: string;
    name: string;
    statusCode: string;
    paymentTermsCode: string;
    defaultLeadTimeDays: string;
    currencyCode: string;
    isActive: boolean;
  }>({
    id: null,
    rowVersion: null,
    code: "",
    name: "",
    statusCode: "APPROVED",
    paymentTermsCode: "",
    defaultLeadTimeDays: "",
    currencyCode: "",
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
  const [docLinkFormPurpose, setDocLinkFormPurpose] = useState<string>("");
  const [docLinkFormPrimary, setDocLinkFormPrimary] = useState<boolean>(false);

  // ── Data loading ────────────────────────────────────────────────────────────
  const loadAll = useCallback(async () => {
    setLoading(true);
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
      setLoadError(null);
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  const resetRequisitionForm = useCallback(() => {
    setReqDraftLines([emptyRequisitionLine()]);
    setReqEditTarget(null);
    setReqSourceType("REORDER");
    setReqSourceRef("");
    setReqReason("");
    setReqPriority("NORMAL");
  }, []);

  const openRequisitionForm = useCallback(
    (prefill?: RequisitionPrefill) => {
      resetRequisitionForm();
      if (prefill) {
        setReqDraftLines([
          emptyRequisitionLine({
            articleId: prefill.articleId,
            locationId: prefill.locationId ?? 0,
            qty: prefill.quantity ?? 0,
          }),
        ]);
        if (prefill.sourceType) setReqSourceType(prefill.sourceType);
        setReqSourceRef(prefill.sourceRef ?? "");
        setReqReason(prefill.reason ?? "");
      }
      setActiveTab("requisitions");
      setCreateReqOpen(true);
    },
    [resetRequisitionForm],
  );

  const openCreatePoForm = useCallback((requisitionId: number) => {
    setPoDialogReqId(requisitionId);
    setPoSupplierId(0);
    setPoCurrency("");
    setCreatePoOpen(true);
    void listCurrencyExchangeRates()
      .then((rows) =>
        setRateCurrencies(
          [...new Set(rows.filter((r) => r.is_active === 1).map((r) => r.from_currency))].sort(),
        ),
      )
      .catch(() => setRateCurrencies([]));
  }, []);

  useImperativeHandle(ref, () => ({
    openCreateRequisition: (prefill?: RequisitionPrefill) => openRequisitionForm(prefill),
    openCreatePo: () => {
      setActiveTab("purchase-orders");
      const approved = requisitions.find((r) => r.status === "APPROVED");
      openCreatePoForm(approved?.id ?? 0);
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

  // One-shot requests from the page (article → "create requisition", purchase history → PO).
  useEffect(() => {
    if (!request) return;
    if (request.kind === "create-requisition") {
      openRequisitionForm({ articleId: request.articleId, locationId: request.locationId });
    } else {
      setActiveTab("purchase-orders");
      setSelectedPoId(request.purchaseOrderId);
      setPoDetailOpen(true);
    }
    onRequestHandled?.();
  }, [request, onRequestHandled, openRequisitionForm]);

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
    let cancelled = false;
    setReqLinesError(null);
    void listInventoryProcurementRequisitionLines(selectedReqId)
      .then((rows) => {
        if (!cancelled) setReqLines(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setReqLinesError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedReqId, reqLinesVersion]);

  useEffect(() => {
    if (!selectedGrId) {
      setGrLines([]);
      return;
    }
    let cancelled = false;
    setGrLinesLoading(true);
    setGrLinesError(null);
    void listInventoryGoodsReceiptLines(selectedGrId)
      .then((rows) => {
        if (!cancelled) setGrLines(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setGrLinesError(err);
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
    setSupplierDetailError(null);
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
      .catch((err: unknown) => setSupplierDetailError(err))
      .finally(() => setSupplierScorecardLoading(false));
  }, [selectedSupplierId, supplierDetailVersion]);

  const selectedRequisition = useMemo(
    () => requisitions.find((row) => row.id === selectedReqId) ?? null,
    [requisitions, selectedReqId],
  );

  /**
   * Runs a mutation then reloads. Resolves to false when the mutation failed: the global IPC
   * toast already explains why, so callers only decide whether to keep their dialog open.
   */
  /** Why the last mutation was refused (translated), for callers that announce it (board moves). */
  const lastSaveErrorRef = useRef<string | null>(null);
  const runSaving = useCallback(
    async (work: () => Promise<void>): Promise<boolean> => {
      setSaving(true);
      lastSaveErrorRef.current = null;
      try {
        await work();
      } catch (err) {
        lastSaveErrorRef.current = describeAppError(err).title;
        return false;
      } finally {
        setSaving(false);
      }
      await loadAll();
      return true;
    },
    [loadAll],
  );

  const openRequisitionDetails = useCallback((reqId: number) => {
    setSelectedReqId(reqId);
    setReqDetailOpen(true);
  }, []);

  /** Row-level requisition transition (list menus, independent of the open detail). */
  const runRequisitionTransition = useCallback(
    (row: ProcurementRequisition, nextStatus: string, reason?: string) =>
      runSaving(async () => {
        await transitionInventoryProcurementRequisition({
          requisition_id: row.id,
          expected_row_version: row.row_version,
          next_status: nextStatus,
          reason: reason ?? null,
        });
      }),
    [runSaving],
  );

  const runPurchaseOrderTransition = useCallback(
    (row: PurchaseOrder, nextStatus: string) =>
      runSaving(async () => {
        await transitionInventoryPurchaseOrder({
          purchase_order_id: row.id,
          expected_row_version: row.row_version,
          next_status: nextStatus,
        });
      }),
    [runSaving],
  );

  /** Cancelling / closing a requisition is irreversible: ask first. */
  const confirmRequisitionDecline = useCallback(
    (row: ProcurementRequisition, target: "CANCELLED" | "CLOSED") => {
      const kind = target === "CANCELLED" ? "confirmCancel" : "confirmClose";
      setPendingConfirm({
        title: t(`procurement.requisitions.${kind}.title`),
        description: t(`procurement.requisitions.${kind}.description`, {
          number: row.req_number,
        }),
        confirmLabel: t(`procurement.requisitions.${kind}.confirm`),
        run: () => runRequisitionTransition(row, target),
      });
    },
    [runRequisitionTransition, t],
  );

  const confirmPurchaseOrderCancel = useCallback(
    (row: PurchaseOrder) =>
      setPendingConfirm({
        title: t("procurement.purchaseOrders.confirmCancel.title"),
        description: t("procurement.purchaseOrders.confirmCancel.description", {
          number: row.po_number,
        }),
        confirmLabel: t("procurement.purchaseOrders.confirmCancel.confirm"),
        run: () => runPurchaseOrderTransition(row, "CANCELLED"),
      }),
    [runPurchaseOrderTransition, t],
  );

  const rejectSelectedRequisition = async () => {
    const reason = rejectReason.trim();
    if (!reason || !selectedRequisition) return;
    const ok = await runRequisitionTransition(selectedRequisition, "REJECTED", reason);
    // On failure the dialog stays open with the typed reason so the user can retry.
    if (!ok) return;
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
    const ok = await runSaving(async () => {
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
    // Keep the capture dialog (and what was typed) open when the transition failed.
    if (ok) setRepairAction(null);
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
      currencyCode: "",
      isActive: true,
    });
    setSupplierEditOpen(true);
  };

  const openEditSupplierForm = (s: InventorySupplierRow) => {
    setSupplierForm({
      id: s.id,
      rowVersion: s.row_version,
      code: s.code,
      name: s.name,
      statusCode: s.status_code,
      paymentTermsCode: s.payment_terms_code ?? "",
      defaultLeadTimeDays: s.default_lead_time_days != null ? String(s.default_lead_time_days) : "",
      currencyCode: s.currency_code ?? "",
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
          // Empty = the company currency (stored as no currency, no conversion).
          currency_code: supplierForm.currencyCode.trim().toUpperCase() || null,
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
    { id: "requisitions", label: t("procurement.requisitions.title") },
    { id: "purchase-orders", label: t("procurement.purchaseOrders.title") },
    { id: "goods-receipts", label: t("procurement.goodsReceipts.title") },
    { id: "repairables", label: t("procurement.repairables.title") },
    { id: "suppliers", label: t("procurement.suppliers.title") },
    { id: "lifecycle", label: t("procurement.lifecycle.title") },
  ];

  const kanbanViewOptions = useMemo<ViewOption<"list" | "kanban">[]>(
    () => [
      { value: "list", label: t("page.viewList"), icon: List },
      { value: "kanban", label: t("page.viewKanban"), icon: Columns3 },
    ],
    [t],
  );

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
      if (row.status === "APPROVED") {
        actions.push({
          id: "create-po",
          label: t("procurement.requisitions.actions.createPo"),
          permission: P.INV_PROCURE,
          onClick: () => openCreatePoForm(row.id),
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
          onClick: () => confirmRequisitionDecline(row, declineTarget),
        });
      }
      return actions;
    },
    [
      confirmRequisitionDecline,
      openCreatePoForm,
      openRequisitionDetails,
      runRequisitionTransition,
      t,
    ],
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
          onClick: () => confirmPurchaseOrderCancel(row),
        });
      }
      return actions;
    },
    [confirmPurchaseOrderCancel, openPoDetails, openReceiveForPo, runPurchaseOrderTransition, t],
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

  // ── Status chips ────────────────────────────────────────────────────────────
  const procurementStatusChip = useCallback(
    (status: string) => (
      <StatusChip
        label={t(`procurement.statuses.${status}`, { defaultValue: status })}
        tone={toneOf(PROCUREMENT_STATUS_TONE, status)}
        dot
      />
    ),
    [t],
  );
  const repairableStatusChip = useCallback(
    (status: string) => (
      <StatusChip
        label={t(`procurement.repairableStatuses.${status}`, { defaultValue: status })}
        tone={toneOf(REPAIRABLE_STATUS_TONE, status)}
        dot
      />
    ),
    [t],
  );
  const priorityChip = useCallback(
    (priority: string | null) =>
      priority ? (
        <StatusChip
          label={priorityLabel(priority)}
          tone={toneOf(PURCHASE_PRIORITY_TONE, priority)}
        />
      ) : (
        "—"
      ),
    [priorityLabel],
  );
  const supplierStatusChip = useCallback(
    (status: string) => (
      <StatusChip
        label={t(`procurement.suppliers.status.${status}`, { defaultValue: status })}
        tone={toneOf(SUPPLIER_STATUS_TONE, status)}
      />
    ),
    [t],
  );
  /** Status label of any procurement entity (requisition, PO, receipt, repairable). */
  const entityStatusLabel = useCallback(
    (entityType: string, status: string | null) => {
      if (!status) return "—";
      const repairable = entityType.toLowerCase().includes("repair");
      return repairable
        ? t(`procurement.repairableStatuses.${status}`, { defaultValue: status })
        : t(`procurement.statuses.${status}`, { defaultValue: status });
    },
    [t],
  );

  // ── Column definitions ──────────────────────────────────────────────────────
  const reqColumns: ColumnDef<ProcurementRequisitionRow>[] = useMemo(
    () => [
      {
        accessorKey: "req_number",
        header: t("procurement.requisitions.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.req_number}</span>,
      },
      {
        accessorKey: "purchase_priority",
        header: t("procurement.requisition.priority"),
        cell: ({ row }) => priorityChip(row.original.purchase_priority),
      },
      {
        accessorKey: "status",
        header: t("procurement.requisitions.columns.status"),
        cell: ({ row }) => procurementStatusChip(row.original.status),
      },
      {
        id: "source",
        accessorFn: (req) => sourceLabel(req.demand_source_type),
        header: t("procurement.requisitions.columns.source"),
        cell: ({ row }) => (
          <span className="text-xs">
            {sourceLabel(row.original.demand_source_type)}
            {row.original.demand_source_ref ? (
              <span className="text-text-muted"> · {row.original.demand_source_ref}</span>
            ) : null}
          </span>
        ),
      },
      {
        id: "requested_by",
        accessorFn: (req) => req.requested_by_name ?? "",
        header: t("procurement.requisitions.columns.requestedBy"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.requested_by_name ?? "—"}</span>
        ),
      },
      {
        accessorKey: "updated_at",
        header: t("procurement.requisitions.columns.updated"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{formatDateTime(row.original.updated_at)}</span>
        ),
      },
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
    [formatDateTime, priorityChip, procurementStatusChip, requisitionRowActions, sourceLabel, t],
  );

  const poColumns: ColumnDef<PurchaseOrderRow>[] = useMemo(
    () => [
      {
        accessorKey: "po_number",
        header: t("procurement.purchaseOrders.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.po_number}</span>,
      },
      {
        accessorKey: "status",
        header: t("procurement.purchaseOrders.columns.status"),
        cell: ({ row }) => procurementStatusChip(row.original.status),
      },
      {
        id: "supplier",
        accessorFn: (po) => po.supplier_name ?? po.supplier_company_name ?? "",
        header: t("procurement.purchaseOrders.columns.supplier"),
        cell: ({ row }) =>
          row.original.supplier_name ??
          row.original.supplier_company_name ??
          t("procurement.purchaseOrders.noSupplier"),
      },
      {
        id: "ordered_by",
        accessorFn: (po) => po.ordered_by_name ?? "",
        header: t("procurement.purchaseOrders.columns.orderedBy"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{row.original.ordered_by_name ?? "—"}</span>
        ),
      },
      {
        accessorKey: "expected_delivery_date",
        header: t("procurement.purchaseOrders.columns.expectedDelivery"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {dateLabel(row.original.expected_delivery_date)}
          </span>
        ),
      },
      {
        accessorKey: "updated_at",
        header: t("procurement.purchaseOrders.columns.updated"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{formatDateTime(row.original.updated_at)}</span>
        ),
      },
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
    [dateLabel, formatDateTime, procurementStatusChip, purchaseOrderRowActions, t],
  );

  const repairColumns: ColumnDef<RepairableOrder>[] = useMemo(
    () => [
      {
        accessorKey: "order_code",
        header: t("procurement.repairables.columns.order"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.order_code}</span>,
      },
      {
        id: "article",
        accessorFn: (order) => order.article_code,
        header: t("procurement.repairables.columns.article"),
        cell: ({ row }) => formatAssetLabel(row.original.article_code, row.original.article_name),
      },
      { accessorKey: "source_location_code", header: t("procurement.repairables.columns.source") },
      {
        id: "vendor",
        header: t("procurement.repairables.columns.vendor"),
        cell: ({ row }) => formatOrDash(row.original.vendor_supplier_name),
      },
      {
        accessorKey: "status",
        header: t("procurement.repairables.columns.status"),
        cell: ({ row }) => repairableStatusChip(row.original.status),
      },
      {
        accessorKey: "updated_at",
        header: t("procurement.repairables.columns.updated"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{formatDateTime(row.original.updated_at)}</span>
        ),
      },
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
    [formatDateTime, repairableRowActions, repairableStatusChip, t],
  );

  const grColumns: ColumnDef<GoodsReceipt>[] = useMemo(
    () => [
      {
        accessorKey: "gr_number",
        header: t("procurement.goodsReceipts.columns.number"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.gr_number}</span>,
      },
      {
        id: "purchase_order",
        header: t("procurement.goodsReceipts.columns.po"),
        cell: ({ row }) => {
          const po = purchaseOrderById.get(row.original.purchase_order_id);
          if (!po) return "—";
          return (
            <Button
              variant="link"
              className="h-auto p-0 font-mono"
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
        cell: ({ row }) => procurementStatusChip(row.original.status),
      },
      {
        accessorKey: "posting_state",
        header: t("procurement.goodsReceipts.columns.posting"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`procurement.postingStates.${row.original.posting_state}`, {
              defaultValue: row.original.posting_state,
            })}
            tone={toneOf(POSTING_STATE_TONE, row.original.posting_state)}
          />
        ),
      },
      {
        accessorKey: "received_at",
        header: t("procurement.goodsReceipts.columns.receivedAt"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.received_at ? formatDateTime(row.original.received_at) : "—"}
          </span>
        ),
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
    [formatDateTime, openGoodsReceipt, openPoDetails, procurementStatusChip, purchaseOrderById, t],
  );

  const eventColumns: ColumnDef<InventoryStateEvent>[] = useMemo(
    () => [
      {
        accessorKey: "entity_type",
        header: t("procurement.lifecycle.columns.entity"),
        cell: ({ row }) =>
          t(`procurement.lifecycle.entities.${row.original.entity_type.toLowerCase()}`, {
            defaultValue: row.original.entity_type,
          }),
      },
      {
        id: "transition",
        header: t("procurement.lifecycle.columns.transition"),
        cell: ({ row }) => (
          <span className="text-xs">
            {entityStatusLabel(row.original.entity_type, row.original.from_status)}
            <span aria-hidden className="mx-1">
              →
            </span>
            <span className="sr-only">{t("procurement.lifecycle.to")}</span>
            {entityStatusLabel(row.original.entity_type, row.original.to_status)}
          </span>
        ),
      },
      {
        accessorKey: "reason",
        header: t("procurement.lifecycle.columns.reason"),
        cell: ({ row }) => formatOrDash(row.original.note ?? row.original.reason),
      },
      {
        accessorKey: "changed_at",
        header: t("procurement.lifecycle.columns.changedAt"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">{formatDateTime(row.original.changed_at)}</span>
        ),
      },
    ],
    [entityStatusLabel, formatDateTime, t],
  );

  const supplierColumns: ColumnDef<InventorySupplierRow>[] = useMemo(
    () => [
      { accessorKey: "code", header: t("procurement.suppliers.columns.code") },
      { accessorKey: "name", header: t("procurement.suppliers.columns.name") },
      {
        accessorKey: "status_code",
        header: t("procurement.suppliers.columns.status"),
        cell: ({ row }) => supplierStatusChip(row.original.status_code),
      },
      {
        accessorKey: "default_lead_time_days",
        header: t("procurement.suppliers.columns.leadTime"),
        cell: ({ row }) => row.original.default_lead_time_days ?? "—",
      },
    ],
    [supplierStatusChip, t],
  );

  // ── Derived lists ───────────────────────────────────────────────────────────
  const approvedRequisitions = useMemo(
    () => requisitions.filter((r) => r.status === "APPROVED"),
    [requisitions],
  );

  const filteredRequisitions = useMemo(() => {
    const term = reqSearch.trim().toLowerCase();
    return requisitions.filter((row) => {
      if (reqStatusFilter === OPEN) {
        if (!(dashboard?.open_requisition_statuses ?? []).includes(row.status)) return false;
      } else if (reqStatusFilter !== ALL && row.status !== reqStatusFilter) return false;
      return includesTerm(
        term,
        row.req_number,
        row.demand_source_ref,
        sourceLabel(row.demand_source_type),
        row.requested_by_name,
      );
    });
  }, [requisitions, reqSearch, reqStatusFilter, sourceLabel, dashboard]);

  const filteredPurchaseOrders = useMemo(() => {
    const term = poSearch.trim().toLowerCase();
    return purchaseOrders.filter((row) => {
      if (poStatusFilter === OPEN) {
        if (!(dashboard?.open_po_statuses ?? []).includes(row.status)) return false;
      } else if (poStatusFilter === LATE) {
        // Same rule as the "late" tile: a late-able status and an expected delivery before now.
        if (!(dashboard?.late_po_statuses ?? []).includes(row.status)) return false;
        if (!row.expected_delivery_date) return false;
        if (new Date(row.expected_delivery_date).getTime() >= Date.now()) return false;
      } else if (poStatusFilter !== ALL && row.status !== poStatusFilter) return false;
      if (poSupplierFilter !== ALL && String(row.supplier_id) !== poSupplierFilter) return false;
      return includesTerm(term, row.po_number, row.supplier_company_name, row.supplier_name);
    });
  }, [purchaseOrders, poSearch, poStatusFilter, poSupplierFilter, dashboard]);

  const reqKanbanColumns = useMemo<KanbanColumn[]>(
    () =>
      REQ_KANBAN_STATUSES.map((status) => ({
        id: status,
        label: t(`procurement.statuses.${status}`, { defaultValue: status }),
        tone: toneOf(PROCUREMENT_STATUS_TONE, status),
      })),
    [t],
  );

  const reqKanbanCards = useMemo<KanbanCard[]>(
    () =>
      filteredRequisitions.map((req) => ({
        id: String(req.id),
        columnId: req.status,
        code: req.req_number,
        title: [sourceLabel(req.demand_source_type), req.demand_source_ref]
          .filter(Boolean)
          .join(" · "),
        ...(req.purchase_priority
          ? {
              badges: [
                {
                  label: priorityLabel(req.purchase_priority),
                  tone: toneOf(PURCHASE_PRIORITY_TONE, req.purchase_priority),
                },
              ],
            }
          : {}),
        selected: selectedReqId === req.id,
      })),
    [filteredRequisitions, priorityLabel, selectedReqId, sourceLabel],
  );

  const poKanbanColumns = useMemo<KanbanColumn[]>(
    () =>
      PO_KANBAN_STATUSES.map((status) => ({
        id: status,
        label: t(`procurement.statuses.${status}`, { defaultValue: status }),
        tone: toneOf(PROCUREMENT_STATUS_TONE, status),
      })),
    [t],
  );

  const poKanbanCards = useMemo<KanbanCard[]>(
    () =>
      filteredPurchaseOrders.map((po) => ({
        id: String(po.id),
        columnId: po.status,
        code: po.po_number,
        title:
          po.supplier_name ??
          po.supplier_company_name ??
          t("procurement.purchaseOrders.noSupplier"),
        selected: selectedPoId === po.id,
      })),
    [filteredPurchaseOrders, selectedPoId, t],
  );

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
        options: [
          { value: OPEN, label: t("procurement.filters.open") },
          ...REQ_KANBAN_STATUSES.map((status) => ({
            value: status,
            label: t(`procurement.statuses.${status}`, { defaultValue: status }),
          })),
        ],
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
        options: [
          { value: OPEN, label: t("procurement.filters.open") },
          { value: LATE, label: t("procurement.filters.late") },
          ...PO_KANBAN_STATUSES.map((status) => ({
            value: status,
            label: t(`procurement.statuses.${status}`, { defaultValue: status }),
          })),
        ],
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
  const requisitionLinesEditor = (
    <RequisitionLinesEditor
      lines={reqDraftLines}
      onChange={setReqDraftLines}
      articles={articles}
      locations={locations}
      disabled={saving}
    />
  );

  const requisitionForm = (
    <div className="grid gap-3 md:grid-cols-2">
      <div className="md:col-span-2">{requisitionLinesEditor}</div>
      <div className="space-y-1">
        <Label htmlFor="inv-req-priority">{t("procurement.requisition.priority")}</Label>
        <Select value={reqPriority} onValueChange={setReqPriority}>
          <SelectTrigger id="inv-req-priority">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {PURCHASE_PRIORITIES.map((p) => (
              <SelectItem key={p} value={p}>
                {priorityLabel(p)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <Label htmlFor="inv-req-source">{t("procurement.requisitionForm.source")}</Label>
        <Select value={reqSourceType} onValueChange={setReqSourceType}>
          <SelectTrigger id="inv-req-source">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {MANUAL_DEMAND_SOURCES.map((code) => (
              <SelectItem key={code} value={code}>
                {sourceLabel(code)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-text-muted">{t("procurement.requisitionForm.sourceHint")}</p>
      </div>
      <div className="space-y-1">
        <Label htmlFor="inv-req-source-ref">{t("procurement.requisitionForm.sourceRef")}</Label>
        <Input
          id="inv-req-source-ref"
          value={reqSourceRef}
          onChange={(e) => setReqSourceRef(e.target.value)}
        />
      </div>
      <div className="space-y-1 md:col-span-2">
        <Label htmlFor="inv-req-reason">{t("procurement.requisitionForm.reason")}</Label>
        <Input
          id="inv-req-reason"
          value={reqReason}
          onChange={(e) => setReqReason(e.target.value)}
        />
      </div>
    </div>
  );

  const reqLinesBlocker = requisitionLinesBlocker(reqDraftLines);
  const reqFormBlocker = reqLinesBlocker
    ? t(`procurement.requisitionForm.blockers.${reqLinesBlocker}`)
    : null;

  const submitRequisition = async () => {
    const editing = reqEditTarget;
    const ok = await runSaving(async () => {
      if (editing) {
        await replaceInventoryRequisitionLines({
          requisition_id: editing.id,
          expected_row_version: editing.row_version,
          lines: draftLinesToInput(reqDraftLines),
        });
      } else {
        // The requester is recorded by the backend from the signed-in session.
        await createInventoryMultiLineRequisition({
          lines: draftLinesToInput(reqDraftLines),
          demand_source_type: reqSourceType,
          demand_source_ref: reqSourceRef.trim() || null,
          purchase_priority: reqPriority || null,
          reason: reqReason.trim() || null,
        });
      }
    });
    if (!ok) return;
    setCreateReqOpen(false);
    resetRequisitionForm();
    if (editing) setReqLinesVersion((v) => v + 1);
  };

  /** Opens the lines of a draft requisition for editing. */
  const openEditRequisitionLines = (req: ProcurementRequisitionRow) => {
    setReqDraftLines(
      reqLines.map((line) =>
        emptyRequisitionLine({
          articleId: line.article_id,
          locationId: line.preferred_location_id ?? 0,
          qty: line.requested_qty,
          estimatedCost: line.estimated_unit_cost != null ? String(line.estimated_unit_cost) : "",
        }),
      ),
    );
    setReqEditTarget(req);
    setReqDetailOpen(false);
    setCreateReqOpen(true);
  };

  const selectedSupplier = useMemo(
    () => inventorySuppliers.find((s) => s.id === selectedSupplierId) ?? null,
    [inventorySuppliers, selectedSupplierId],
  );

  // ── Dashboard KPI strip ─────────────────────────────────────────────────────
  const kpiStrip = dashboard ? (
    <div className="mb-4 grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-5">
      <KpiTile
        label={t("procurement.dashboard.openRequisitions")}
        value={formatNumber(dashboard.open_requisitions)}
        onOpen={() => {
          setActiveTab("requisitions");
          setReqStatusFilter(OPEN);
        }}
      />
      <KpiTile
        label={t("procurement.dashboard.pendingApprovals")}
        value={formatNumber(dashboard.pending_approval_pos)}
        {...(dashboard.pending_approval_pos > 0 ? { tone: "attention" as const } : {})}
        onOpen={() => {
          setActiveTab("purchase-orders");
          setPoStatusFilter("SUBMITTED");
        }}
      />
      <KpiTile
        label={t("procurement.dashboard.openPos")}
        value={formatNumber(dashboard.open_pos)}
        onOpen={() => {
          setActiveTab("purchase-orders");
          setPoStatusFilter(OPEN);
        }}
      />
      <KpiTile
        label={t("procurement.dashboard.latePos")}
        value={formatNumber(dashboard.overdue_pos)}
        {...(dashboard.overdue_pos > 0 ? { tone: "danger" as const } : {})}
        onOpen={() => {
          setActiveTab("purchase-orders");
          setPoStatusFilter(LATE);
        }}
      />
      <KpiTile
        label={t("procurement.dashboard.pendingReceipts")}
        value={formatNumber(dashboard.pending_receipts)}
        hint={t("procurement.dashboard.receivingTodayHint", {
          count: dashboard.receiving_today_count,
        })}
        onOpen={() => {
          setActiveTab("purchase-orders");
          setPoStatusFilter("APPROVED");
        }}
      />
      <KpiTile
        label={t("procurement.dashboard.stockouts")}
        value={formatNumber(dashboard.low_stock_articles)}
        hint={t("procurement.dashboard.stockoutsHint")}
        {...(dashboard.low_stock_articles > 0 ? { tone: "warning" as const } : {})}
        onOpen={() => document.getElementById(RECOMMENDATIONS_ID)?.scrollIntoView()}
      />
      <KpiTile
        label={t("procurement.dashboard.criticalStockouts")}
        value={formatNumber(dashboard.critical_low_stock_articles)}
        hint={t("procurement.dashboard.criticalStockoutsHint")}
        {...(dashboard.critical_low_stock_articles > 0 ? { tone: "danger" as const } : {})}
        onOpen={() => document.getElementById(RECOMMENDATIONS_ID)?.scrollIntoView()}
      />
      <KpiTile
        label={t("procurement.dashboard.repairablesInRepair")}
        value={formatNumber(dashboard.repairables_in_repair)}
        onOpen={() => setActiveTab("repairables")}
      />
      <KpiTile
        label={t("procurement.dashboard.activeSuppliers")}
        value={formatNumber(dashboard.active_suppliers_count)}
        onOpen={() => setActiveTab("suppliers")}
      />
    </div>
  ) : null;

  /** Next step of the open requisition, shown next to its lifecycle buttons. */
  const requisitionStepHint = selectedRequisition
    ? t(`procurement.requisitions.hints.${selectedRequisition.status}`, {
        defaultValue: t("procurement.requisitions.hints.terminal"),
      })
    : null;

  return (
    <div className="space-y-4">
      {/* A failed load replaces the procurement content; dialogs below stay usable. */}
      {loadError ? (
        <ErrorState
          error={loadError}
          title={t("procurement.loadError")}
          onRetry={() => void loadAll()}
          retrying={loading}
        />
      ) : (
        <>
          {kpiStrip}
          <ProcurementAlertCards
            alerts={alerts}
            onNavigate={(tab) => {
              setActiveTab(tab);
              if (tab === "purchase-orders") setPoStatusFilter(ALL);
              if (tab === "repairables") setRepairStatusFilter(ALL);
            }}
          />

          <ProcurementRecommendationsPanel
            id={RECOMMENDATIONS_ID}
            onCreateRequisitionPrefill={(rec, reason) =>
              openRequisitionForm({
                articleId: rec.article_id,
                quantity: rec.suggested_reorder_qty,
                sourceType: "REORDER",
                sourceRef: rec.warehouse_code,
                reason,
              })
            }
            onCreated={() => loadAll()}
          />

          <div className="grid gap-4 lg:grid-cols-[220px_minmax(0,1fr)]">
            {/* Sub-tab sidebar */}
            <nav className="rounded-md border p-2" aria-label={t("procurement.sectionsLabel")}>
              <div className="space-y-1">
                {subTabs.map((tab) => (
                  <button
                    key={tab.id}
                    type="button"
                    aria-current={activeTab === tab.id ? "page" : undefined}
                    onClick={() => setActiveTab(tab.id)}
                    className={cn(
                      "min-h-11 w-full rounded-md px-3 py-2 text-left text-sm transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      activeTab === tab.id
                        ? "bg-primary text-primary-foreground"
                        : "hover:bg-muted",
                    )}
                  >
                    {tab.label}
                  </button>
                ))}
              </div>
            </nav>

            <div className="min-w-0 space-y-4">
              {/* ── REQUISITIONS ── */}
              {activeTab === "requisitions" ? (
                <section className="rounded-md border p-4">
                  <h3 className="mb-2 text-sm font-semibold">
                    {t("procurement.requisitions.title")}
                  </h3>
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
                      <ViewToggle
                        value={reqView}
                        options={kanbanViewOptions}
                        onChange={setReqView}
                      />
                    ) : null}
                  </div>
                  {reqView === "kanban" ? (
                    <div className="mb-3">
                      {canProcure ? (
                        <p className="mb-2 px-1 text-xs text-text-muted">
                          {t("procurement.boardMoveHint")}
                        </p>
                      ) : null}
                      <KanbanBoard
                        columns={reqKanbanColumns}
                        cards={reqKanbanCards}
                        enableDrag={canProcure}
                        canDrop={(_card, from, to) => isBoardOneClickMove(from, to)}
                        onDrop={async (card, _from, to) => {
                          const row = requisitions.find((r) => String(r.id) === card.id);
                          if (!row) return false;
                          const ok = await runRequisitionTransition(row, to);
                          return ok
                            ? true
                            : { ok: false, reason: lastSaveErrorRef.current ?? undefined };
                        }}
                        onCardClick={(card) => openRequisitionDetails(Number(card.id))}
                        emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
                        aria-label={t("procurement.requisitions.boardLabel")}
                      />
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
                                onAction: () => openRequisitionForm(),
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
                </section>
              ) : null}

              {/* ── PURCHASE ORDERS ── */}
              {activeTab === "purchase-orders" ? (
                <section className="rounded-md border p-4">
                  <h3 className="mb-3 text-sm font-semibold">
                    {t("procurement.purchaseOrders.title")}
                  </h3>
                  <div className="mb-3 flex flex-wrap items-center gap-2">
                    <SmartFilterBar
                      className="min-w-0 flex-1"
                      searchPlaceholder={t("procurement.purchaseOrders.searchPlaceholder")}
                      searchValue={poSearchInput}
                      onSearchInputChange={setPoSearchInput}
                      onSearchChange={setPoSearch}
                      filters={poFilterDefs}
                      resultCount={filteredPurchaseOrders.length}
                      onReset={resetPoFilters}
                    />
                    {!viewFromParent ? (
                      <ViewToggle
                        value={reqView}
                        options={kanbanViewOptions}
                        onChange={setReqView}
                      />
                    ) : null}
                  </div>
                  {reqView === "kanban" ? (
                    <div className="mb-3">
                      {canProcure ? (
                        <p className="mb-2 px-1 text-xs text-text-muted">
                          {t("procurement.boardMoveHint")}
                        </p>
                      ) : null}
                      <KanbanBoard
                        columns={poKanbanColumns}
                        cards={poKanbanCards}
                        enableDrag={canProcure}
                        canDrop={(_card, from, to) => isBoardOneClickMove(from, to)}
                        onDrop={async (card, _from, to) => {
                          const row = purchaseOrders.find((r) => String(r.id) === card.id);
                          if (!row) return false;
                          const ok = await runPurchaseOrderTransition(row, to);
                          return ok
                            ? true
                            : { ok: false, reason: lastSaveErrorRef.current ?? undefined };
                        }}
                        onCardClick={(card) => openPoDetails(Number(card.id))}
                        emptyColumnLabel={t("kanban.emptyColumn", { ns: "common" })}
                        aria-label={t("procurement.purchaseOrders.boardLabel")}
                      />
                    </div>
                  ) : !loading && purchaseOrders.length === 0 ? (
                    <EmptyState
                      title={t("procurement.purchaseOrders.empty")}
                      description={t("procurement.purchaseOrders.emptyDescription")}
                      {...(canProcure
                        ? {
                            actionLabel: t("procurement.purchaseOrders.create"),
                            onAction: () => openCreatePoForm(approvedRequisitions[0]?.id ?? 0),
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
                </section>
              ) : null}

              {/* ── REPAIRABLES ── */}
              {activeTab === "repairables" ? (
                <section className="rounded-md border p-4">
                  <h3 className="mb-2 text-sm font-semibold">
                    {t("procurement.repairables.title")}
                  </h3>
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
                </section>
              ) : null}

              {/* ── GOODS RECEIPTS ── */}
              {activeTab === "goods-receipts" ? (
                <section className="rounded-md border p-4">
                  <h3 className="mb-3 text-sm font-semibold">
                    {t("procurement.goodsReceipts.title")}
                  </h3>
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
                      {grLinesError ? (
                        <ErrorState
                          error={grLinesError}
                          onRetry={() => {
                            const id = selectedGrId;
                            setSelectedGrId(null);
                            window.setTimeout(() => setSelectedGrId(id), 0);
                          }}
                        />
                      ) : grLinesLoading ? (
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
                                  {qtyLabel(line.received_qty)}
                                </TableCell>
                                <TableCell className="text-right tabular-nums">
                                  {qtyLabel(line.accepted_qty)}
                                </TableCell>
                                <TableCell className="text-right tabular-nums">
                                  {qtyLabel(line.rejected_qty)}
                                </TableCell>
                                <TableCell>{formatOrDash(line.rejection_reason)}</TableCell>
                              </TableRow>
                            ))}
                          </TableBody>
                        </Table>
                      )}
                    </div>
                  ) : null}
                </section>
              ) : null}

              {/* ── SUPPLIERS ── */}
              {activeTab === "suppliers" ? (
                <section className="rounded-md border p-4">
                  <div className="mb-3 flex items-center justify-between">
                    <h3 className="text-sm font-semibold">{t("procurement.suppliers.title")}</h3>
                    <PermissionGate permission={P.INV_MANAGE}>
                      <Button className="h-11" onClick={openNewSupplierForm}>
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
                      isRowSelected={(s) => s.id === selectedSupplierId}
                    />

                    {selectedSupplierId ? (
                      <div className="space-y-4">
                        {/* Supplier header */}
                        <div className="flex items-start justify-between gap-2 rounded-md border p-3">
                          <div className="min-w-0">
                            <div className="font-semibold">
                              {selectedSupplier?.code} — {selectedSupplier?.name}
                            </div>
                            {selectedSupplier ? (
                              <div className="mt-1">
                                {supplierStatusChip(selectedSupplier.status_code)}
                              </div>
                            ) : null}
                          </div>
                          <PermissionGate permission={P.INV_MANAGE}>
                            <div className="flex flex-wrap justify-end gap-2">
                              {selectedSupplier ? (
                                <>
                                  <Button
                                    variant="outline"
                                    className="h-11"
                                    onClick={() => openEditSupplierForm(selectedSupplier)}
                                  >
                                    {t("common:action.edit")}
                                  </Button>
                                  {selectedSupplier.is_active === 1 ? (
                                    <Button
                                      variant="destructive"
                                      className="h-11"
                                      disabled={saving}
                                      onClick={() =>
                                        setPendingConfirm({
                                          title: t("procurement.suppliers.confirmDeactivate.title"),
                                          description: t(
                                            "procurement.suppliers.confirmDeactivate.description",
                                            { name: selectedSupplier.name },
                                          ),
                                          confirmLabel: t(
                                            "procurement.suppliers.confirmDeactivate.confirm",
                                          ),
                                          run: async () => {
                                            const ok = await runSaving(async () => {
                                              await deactivateInventorySupplier(
                                                selectedSupplier.id,
                                                selectedSupplier.row_version,
                                              );
                                            });
                                            if (ok) setSelectedSupplierId(null);
                                            return ok;
                                          },
                                        })
                                      }
                                    >
                                      {t("procurement.suppliers.deactivate")}
                                    </Button>
                                  ) : null}
                                </>
                              ) : null}
                            </div>
                          </PermissionGate>
                        </div>

                        {supplierDetailError ? (
                          <ErrorState
                            error={supplierDetailError}
                            title={t("procurement.suppliers.detailLoadError")}
                            onRetry={() => setSupplierDetailVersion((v) => v + 1)}
                            retrying={supplierScorecardLoading}
                          />
                        ) : null}

                        {/* Scorecard */}
                        {supplierScorecardLoading ? (
                          <div className="rounded-md border p-3 text-sm text-text-muted">
                            {t("common:action.loading")}
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
                                    qtyLabel(supplierScorecard.avg_lead_time_days),
                                  ],
                                  [
                                    t("procurement.suppliers.scorecard.deliveryAccuracy"),
                                    pctLabel(supplierScorecard.delivery_accuracy_pct),
                                  ],
                                  [
                                    t("procurement.suppliers.scorecard.avgPrice"),
                                    moneyLabel(supplierScorecard.avg_price),
                                  ],
                                  [
                                    t("procurement.suppliers.scorecard.openPos"),
                                    formatNumber(supplierScorecard.open_po_count),
                                  ],
                                  [
                                    t("procurement.suppliers.scorecard.completedPos"),
                                    formatNumber(supplierScorecard.completed_po_count),
                                  ],
                                  [
                                    t("procurement.suppliers.scorecard.lastPurchase"),
                                    dateLabel(supplierScorecard.last_purchase_at),
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
                                        {t(
                                          "procurement.suppliers.scorecard.deliveryColumns.article",
                                        )}
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
                                        {t(
                                          "procurement.suppliers.scorecard.deliveryColumns.leadTime",
                                        )}
                                      </TableHead>
                                    </TableRow>
                                  </TableHeader>
                                  <TableBody>
                                    {supplierScorecard.last_deliveries.map((delivery, index) => (
                                      <TableRow
                                        key={`${delivery.po_number}-${delivery.article_code}-${index}`}
                                      >
                                        <TableCell className="px-2 py-1.5 text-xs">
                                          {dateLabel(delivery.received_at)}
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
                                          {qtyLabel(delivery.ordered_qty)}
                                        </TableCell>
                                        <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                          {qtyLabel(delivery.accepted_qty)}
                                        </TableCell>
                                        <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                          {qtyLabel(delivery.rejected_qty)}
                                        </TableCell>
                                        <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                          {qtyLabel(delivery.actual_lead_time_days)}
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
                                      {dateLabel(row.ordered_at)}
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
                                      {qtyLabel(row.ordered_qty)}
                                    </TableCell>
                                    <TableCell className="px-2 py-1.5 text-right text-xs tabular-nums">
                                      {moneyLabel(row.unit_price)}
                                    </TableCell>
                                    <TableCell className="px-2 py-1.5 text-xs">
                                      {procurementStatusChip(row.status)}
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
                                variant="outline"
                                className="h-10"
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
                                  <span className="tabular-nums">
                                    {moneyLabel(price.unit_price)}
                                  </span>
                                  {price.min_order_qty ? (
                                    <span className="text-text-muted">
                                      {t("procurement.suppliers.prices.minQtyValue", {
                                        qty: qtyLabel(price.min_order_qty),
                                      })}
                                    </span>
                                  ) : null}
                                  {price.valid_from ? (
                                    <span className="text-text-muted">
                                      {t("procurement.suppliers.prices.validFromValue", {
                                        date: dateLabel(price.valid_from),
                                      })}
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
                                variant="outline"
                                className="h-10"
                                onClick={() => {
                                  setDocLinkFormRef("");
                                  setDocLinkFormPurpose(documentPurposes.options[0]?.code ?? "");
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
                                  <span className="text-text-muted">
                                    {documentPurposes.labelOf(doc.link_purpose)}
                                  </span>
                                  {doc.is_primary === 1 ? (
                                    <Badge variant="secondary" className="h-4 text-[9px]">
                                      {t("procurement.suppliers.documents.fields.primary")}
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
                </section>
              ) : null}

              {/* ── LIFECYCLE EVENTS ── */}
              {activeTab === "lifecycle" ? (
                <section className="rounded-md border p-4">
                  <h3 className="mb-1 text-sm font-semibold">{t("procurement.lifecycle.title")}</h3>
                  <p className="mb-3 text-xs text-text-muted">{t("procurement.lifecycle.hint")}</p>
                  <div className="mb-3 grid gap-2 md:grid-cols-4">
                    <div className="space-y-1">
                      <Label htmlFor="inv-event-article" className="text-xs">
                        {t("procurement.lifecycle.filters.article")}
                      </Label>
                      <Select value={eventArticleFilter} onValueChange={setEventArticleFilter}>
                        <SelectTrigger id="inv-event-article" className="h-10 text-sm">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="__all__">
                            {t("procurement.lifecycle.filters.allArticles")}
                          </SelectItem>
                          {articles.map((a) => (
                            <SelectItem key={a.id} value={String(a.id)}>
                              {a.article_code}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-1">
                      <Label htmlFor="inv-event-warehouse" className="text-xs">
                        {t("procurement.lifecycle.filters.warehouse")}
                      </Label>
                      <Select value={eventWarehouseFilter} onValueChange={setEventWarehouseFilter}>
                        <SelectTrigger id="inv-event-warehouse" className="h-10 text-sm">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="__all__">
                            {t("procurement.lifecycle.filters.allWarehouses")}
                          </SelectItem>
                          {warehouseOptions.map((w) => (
                            <SelectItem key={w} value={w}>
                              {w}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    <div className="space-y-1">
                      <Label htmlFor="inv-event-from" className="text-xs">
                        {t("procurement.lifecycle.filters.from")}
                      </Label>
                      <Input
                        id="inv-event-from"
                        type="date"
                        value={eventFromDate}
                        onChange={(e) => setEventFromDate(e.target.value)}
                      />
                    </div>
                    <div className="space-y-1">
                      <Label htmlFor="inv-event-to" className="text-xs">
                        {t("procurement.lifecycle.filters.to")}
                      </Label>
                      <Input
                        id="inv-event-to"
                        type="date"
                        value={eventToDate}
                        onChange={(e) => setEventToDate(e.target.value)}
                      />
                    </div>
                  </div>
                  <DataTable
                    columns={eventColumns}
                    data={filteredEvents}
                    isLoading={loading}
                    searchable={false}
                  />
                </section>
              ) : null}
            </div>
          </div>
        </>
      )}

      {/* ── DIALOGS ── */}
      {/* Requisition detail dialog */}
      <Dialog open={reqDetailOpen} onOpenChange={setReqDetailOpen}>
        <DialogContent className="max-h-[85vh] max-w-xl overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="flex flex-wrap items-center gap-2">
              {selectedRequisition ? (
                <>
                  <span className="font-mono">{selectedRequisition.req_number}</span>
                  {procurementStatusChip(selectedRequisition.status)}
                </>
              ) : (
                t("procurement.requisitions.detailTitle")
              )}
            </DialogTitle>
          </DialogHeader>
          {selectedRequisition ? (
            <div className="space-y-3">
              <dl className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 rounded-md border p-3 text-sm">
                <dt className="text-text-muted">{t("procurement.requisitions.fields.source")}</dt>
                <dd>{sourceLabel(selectedRequisition.demand_source_type)}</dd>
                <dt className="text-text-muted">
                  {t("procurement.requisitions.fields.sourceRef")}
                </dt>
                <dd>{selectedRequisition.demand_source_ref ?? "—"}</dd>
                <dt className="text-text-muted">{t("procurement.requisition.priority")}</dt>
                <dd>{priorityChip(selectedRequisition.purchase_priority)}</dd>
                <dt className="text-text-muted">
                  {t("procurement.requisitions.fields.requestedBy")}
                </dt>
                <dd>{selectedRequisition.requested_by_name ?? "—"}</dd>
                <dt className="text-text-muted">
                  {t("procurement.requisitions.fields.createdAt")}
                </dt>
                <dd>{formatDateTime(selectedRequisition.created_at)}</dd>
                <dt className="text-text-muted">
                  {t("procurement.requisitions.fields.updatedAt")}
                </dt>
                <dd>{formatDateTime(selectedRequisition.updated_at)}</dd>
              </dl>
              <div className="rounded-md border p-3 text-sm">
                <div className="mb-2 font-medium">{t("procurement.requisitions.lines.title")}</div>
                {reqLinesError ? (
                  <ErrorState error={reqLinesError} />
                ) : reqLines.length === 0 ? (
                  <div className="text-text-muted">{t("procurement.requisitions.lines.empty")}</div>
                ) : (
                  <>
                    <ul className="space-y-1">
                      {reqLines.map((line) => (
                        <li key={line.id} className="flex flex-wrap items-baseline gap-x-2">
                          <span>
                            {line.article_code} — {line.article_name} ×{" "}
                            <span className="tabular-nums">{qtyLabel(line.requested_qty)}</span>
                          </span>
                          {line.preferred_location_code ? (
                            <span className="text-xs text-text-muted">
                              → {line.preferred_location_code}
                            </span>
                          ) : null}
                          <span className="ml-auto text-xs tabular-nums text-text-muted">
                            {line.estimated_unit_cost != null
                              ? t("procurement.requisitions.lines.estimate", {
                                  cost: moneyLabel(line.estimated_unit_cost),
                                  total: moneyLabel(line.estimated_unit_cost * line.requested_qty),
                                })
                              : t("procurement.requisitions.lines.noEstimate")}
                          </span>
                        </li>
                      ))}
                    </ul>
                    <p className="mt-2 border-t border-surface-border pt-2 text-sm tabular-nums">
                      {selectedRequisition.estimate_complete
                        ? t("procurement.requisitionForm.total", {
                            total: moneyLabel(selectedRequisition.estimated_total ?? 0),
                          })
                        : t("procurement.requisitionForm.totalIncomplete", {
                            total: moneyLabel(selectedRequisition.estimated_total ?? 0),
                          })}
                    </p>
                  </>
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
                  <Timeline
                    items={reqEvents}
                    aria-label={t("procurement.requisitions.history.title")}
                    getEntry={(event) => ({
                      id: String(event.id),
                      title: (
                        <>
                          {event.from_status
                            ? t(`procurement.statuses.${event.from_status}`, {
                                defaultValue: event.from_status,
                              })
                            : "—"}
                          <span aria-hidden className="mx-1">
                            →
                          </span>
                          {t(`procurement.statuses.${event.to_status}`, {
                            defaultValue: event.to_status,
                          })}
                        </>
                      ),
                      timestamp: event.changed_at,
                      ...(event.reason && event.reason !== "requisition_created"
                        ? { description: event.reason }
                        : {}),
                    })}
                  />
                )}
              </div>
              {requisitionStepHint ? (
                <p id="inv-req-step-hint" className="text-xs text-text-muted" role="status">
                  {requisitionStepHint}
                </p>
              ) : null}
            </div>
          ) : (
            <p className="text-sm text-text-muted">{t("procurement.requisitions.noSelection")}</p>
          )}
          <DialogFooter className="flex-wrap gap-2">
            <Button variant="outline" className="h-11" onClick={() => setReqDetailOpen(false)}>
              {t("common:action.close")}
            </Button>
            {selectedRequisition ? (
              <PermissionGate permission={P.INV_PROCURE}>
                <>
                  {selectedRequisition.status === "DRAFT" ? (
                    <Button
                      variant="outline"
                      className="h-11"
                      disabled={saving || reqLinesError !== null}
                      onClick={() => openEditRequisitionLines(selectedRequisition)}
                    >
                      {t("procurement.requisitions.actions.editLines")}
                    </Button>
                  ) : null}
                  <Button
                    variant="outline"
                    className="h-11"
                    aria-describedby="inv-req-step-hint"
                    disabled={saving || selectedRequisition.status !== "DRAFT"}
                    onClick={() => void runRequisitionTransition(selectedRequisition, "SUBMITTED")}
                  >
                    {t("procurement.requisitions.actions.submit")}
                  </Button>
                  <Button
                    className="h-11"
                    aria-describedby="inv-req-step-hint"
                    disabled={saving || selectedRequisition.status !== "SUBMITTED"}
                    onClick={() => void runRequisitionTransition(selectedRequisition, "APPROVED")}
                  >
                    {t("procurement.requisitions.actions.approve")}
                  </Button>
                  {selectedRequisition.status === "SUBMITTED" ? (
                    <Button
                      variant="destructive"
                      className="h-11"
                      disabled={saving}
                      onClick={() => {
                        setRejectReason("");
                        setRejectOpen(true);
                      }}
                    >
                      {t("procurement.requisitions.actions.reject")}
                    </Button>
                  ) : null}
                  {selectedRequisition.status === "APPROVED" ? (
                    <Button
                      className="h-11"
                      disabled={saving}
                      onClick={() => openCreatePoForm(selectedRequisition.id)}
                    >
                      {t("procurement.requisitions.actions.createPo")}
                    </Button>
                  ) : null}
                  {(() => {
                    const target = requisitionDeclineOrCloseTarget(selectedRequisition.status);
                    if (!target) return null;
                    return (
                      <Button
                        variant="destructive"
                        className="h-11"
                        disabled={saving}
                        onClick={() => confirmRequisitionDecline(selectedRequisition, target)}
                      >
                        {target === "CLOSED"
                          ? t("procurement.requisitions.actions.close")
                          : t("procurement.requisitions.actions.cancel")}
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
      <Dialog open={rejectOpen} onOpenChange={(open) => !saving && setRejectOpen(open)}>
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
          <DialogFooter className="gap-2">
            <Button
              variant="outline"
              className="h-11"
              disabled={saving}
              onClick={() => setRejectOpen(false)}
            >
              {t("common:action.cancel")}
            </Button>
            <Button
              variant="destructive"
              className="h-11"
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
      <Dialog
        open={createReqOpen}
        onOpenChange={(open) => {
          setCreateReqOpen(open);
          if (!open) resetRequisitionForm();
        }}
      >
        <DialogContent
          className="max-h-[90vh] max-w-2xl overflow-y-auto"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle>
              {reqEditTarget
                ? t("procurement.requisitionForm.editTitle", { number: reqEditTarget.req_number })
                : t("procurement.requisitionForm.title")}
            </DialogTitle>
          </DialogHeader>
          {reqEditTarget ? requisitionLinesEditor : requisitionForm}
          {reqFormBlocker ? (
            <p id="inv-req-form-blocker" role="status" className="text-xs text-text-muted">
              {reqFormBlocker}
            </p>
          ) : null}
          <DialogFooter className="gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => setCreateReqOpen(false)}
            >
              {t("common:action.cancel")}
            </Button>
            <Button
              type="button"
              className="h-11"
              aria-describedby={reqFormBlocker ? "inv-req-form-blocker" : undefined}
              disabled={saving || reqFormBlocker !== null}
              onClick={() => void submitRequisition()}
            >
              {reqEditTarget
                ? t("procurement.requisitionForm.saveLines")
                : t("procurement.requisitionForm.submit")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Create PO dialog */}
      <Dialog open={createPoOpen} onOpenChange={setCreatePoOpen}>
        <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("procurement.purchaseOrders.create")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3 py-2">
            <div className="space-y-1">
              <Label htmlFor="inv-po-requisition">{t("procurement.poForm.requisition")}</Label>
              <Select
                value={String(poDialogReqId)}
                onValueChange={(v) => setPoDialogReqId(Number(v))}
              >
                <SelectTrigger id="inv-po-requisition">
                  <SelectValue placeholder={t("procurement.poForm.selectRequisition")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("procurement.poForm.selectRequisition")}</SelectItem>
                  {approvedRequisitions.map((req) => (
                    <SelectItem key={req.id} value={String(req.id)}>
                      {req.req_number}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {approvedRequisitions.length === 0 ? (
                <p className="text-xs text-text-muted">{t("procurement.poForm.noApproved")}</p>
              ) : null}
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-po-supplier">{t("procurement.poForm.supplier")}</Label>
              <Select
                value={String(poSupplierId)}
                onValueChange={(v) => {
                  const id = Number(v);
                  setPoSupplierId(id);
                  setPoCurrency(inventorySuppliers.find((s) => s.id === id)?.currency_code ?? "");
                }}
              >
                <SelectTrigger id="inv-po-supplier">
                  <SelectValue placeholder={t("procurement.purchaseOrders.noSupplier")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("procurement.purchaseOrders.noSupplier")}</SelectItem>
                  {nonBlockedSuppliers.map((s) => (
                    <SelectItem key={s.id} value={String(s.id)}>
                      {s.code} — {s.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-text-muted">{t("procurement.poForm.supplierHint")}</p>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-po-currency">{t("procurement.poForm.currency")}</Label>
              <Select
                value={poCurrency || currencyCode}
                onValueChange={(v) => v && setPoCurrency(v === currencyCode ? "" : v)}
              >
                <SelectTrigger id="inv-po-currency">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {[
                    ...new Set([
                      currencyCode,
                      ...(poCurrency ? [poCurrency] : []),
                      ...rateCurrencies,
                    ]),
                  ].map((code) => (
                    <SelectItem key={code} value={code}>
                      {code === currencyCode
                        ? t("procurement.poForm.currencyTenant", { code })
                        : code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-text-muted">{t("procurement.poForm.currencyHint")}</p>
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => setCreatePoOpen(false)}
            >
              {t("common:action.cancel")}
            </Button>
            <Button
              type="button"
              className="h-11"
              disabled={
                saving ||
                poDialogReqId <= 0 ||
                !approvedRequisitions.some((r) => r.id === poDialogReqId)
              }
              onClick={() =>
                void (async () => {
                  const created = { id: null as number | null };
                  const ok = await runSaving(async () => {
                    const po = await createInventoryPurchaseOrderFromRequisition({
                      requisition_id: poDialogReqId,
                      supplier_id: poSupplierId > 0 ? poSupplierId : null,
                      currency_code: poCurrency || null,
                      // The buyer is recorded by the backend from the signed-in session.
                      actor_id: null,
                    });
                    created.id = po.id;
                  });
                  if (!ok) return;
                  setCreatePoOpen(false);
                  setReqDetailOpen(false);
                  if (created.id != null) {
                    setActiveTab("purchase-orders");
                    openPoDetails(created.id);
                  }
                })()
              }
            >
              {t("procurement.poForm.submit")}
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
        articles={articles}
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
          <div className="grid gap-3 md:grid-cols-2">
            <div className="space-y-1">
              <Label htmlFor="inv-repair-article">
                {t("procurement.repairables.form.article")}
              </Label>
              <Select
                value={String(repairArticleId)}
                onValueChange={(v) => setRepairArticleId(Number(v))}
              >
                <SelectTrigger id="inv-repair-article">
                  <SelectValue placeholder={t("procurement.repairables.form.selectArticle")} />
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
              <Label htmlFor="inv-repair-source">
                {t("procurement.repairables.form.sourceLocation")}
              </Label>
              <Select
                value={String(repairSourceLocationId)}
                onValueChange={(v) => setRepairSourceLocationId(Number(v))}
              >
                <SelectTrigger id="inv-repair-source">
                  <SelectValue placeholder={t("procurement.repairables.form.selectSource")} />
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
              <Label htmlFor="inv-repair-return">
                {t("procurement.repairables.form.returnLocation")}
              </Label>
              <Select
                value={String(repairReturnLocationId)}
                onValueChange={(v) => setRepairReturnLocationId(Number(v))}
              >
                <SelectTrigger id="inv-repair-return">
                  <SelectValue placeholder={t("procurement.repairables.form.noReturnLocation")} />
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
              <Label htmlFor="inv-repair-vendor">{t("procurement.repairables.form.vendor")}</Label>
              <Select
                value={String(repairVendorId)}
                onValueChange={(v) => setRepairVendorId(Number(v))}
              >
                <SelectTrigger id="inv-repair-vendor">
                  <SelectValue placeholder={t("procurement.repairables.form.noVendor")} />
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
                <div className="text-text-muted">
                  {t("procurement.repairables.form.balancesEmpty")}
                </div>
              ) : (
                <ul className="space-y-1">
                  {repairBalances.map((b) => (
                    <li key={`${b.id}-${b.location_id}`}>
                      {b.warehouse_code}/{b.location_code}:{" "}
                      {t("procurement.repairables.form.balanceRow", {
                        onHand: qtyLabel(b.on_hand_qty),
                        reserved: qtyLabel(b.reserved_qty),
                        available: qtyLabel(b.available_qty),
                      })}
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : null}
          <DialogFooter className="gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => setRepairOpen(false)}
            >
              {t("procurement.repairables.form.cancel")}
            </Button>
            <Button
              type="button"
              className="h-11"
              disabled={
                saving || repairArticleId <= 0 || repairSourceLocationId <= 0 || repairQty <= 0
              }
              onClick={() =>
                void (async () => {
                  const ok = await runSaving(async () => {
                    await createInventoryRepairableOrder({
                      article_id: repairArticleId,
                      quantity: repairQty,
                      source_location_id: repairSourceLocationId,
                      return_location_id:
                        repairReturnLocationId > 0 ? repairReturnLocationId : null,
                      reason: repairReason.trim() || null,
                      serial_number: repairSerial.trim() || null,
                      vendor_supplier_id: repairVendorId > 0 ? repairVendorId : null,
                    });
                  });
                  if (!ok) return;
                  setRepairArticleId(0);
                  setRepairQty(0);
                  setRepairSourceLocationId(0);
                  setRepairReturnLocationId(0);
                  setRepairReason("");
                  setRepairSerial("");
                  setRepairVendorId(0);
                  setRepairOpen(false);
                })()
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
              <Label htmlFor="inv-supplier-code">{t("procurement.suppliers.fields.code")}</Label>
              <Input
                id="inv-supplier-code"
                value={supplierForm.code}
                onChange={(e) => setSupplierForm((s) => ({ ...s, code: e.target.value }))}
              />
            </div>
            <div className="space-y-1 md:col-span-1">
              <Label htmlFor="inv-supplier-name">{t("procurement.suppliers.fields.name")}</Label>
              <Input
                id="inv-supplier-name"
                value={supplierForm.name}
                onChange={(e) => setSupplierForm((s) => ({ ...s, name: e.target.value }))}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-supplier-status">
                {t("procurement.suppliers.fields.status")}
              </Label>
              <Select
                value={supplierForm.statusCode}
                onValueChange={(v) => setSupplierForm((s) => ({ ...s, statusCode: v }))}
              >
                <SelectTrigger id="inv-supplier-status">
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
              <Label htmlFor="inv-supplier-lead">
                {t("procurement.suppliers.fields.leadTime")}
              </Label>
              <Input
                id="inv-supplier-lead"
                type="number"
                min={0}
                value={supplierForm.defaultLeadTimeDays}
                onChange={(e) =>
                  setSupplierForm((s) => ({ ...s, defaultLeadTimeDays: e.target.value }))
                }
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-supplier-terms">
                {t("procurement.suppliers.fields.paymentTerms")}
              </Label>
              <Input
                id="inv-supplier-terms"
                value={supplierForm.paymentTermsCode}
                onChange={(e) =>
                  setSupplierForm((s) => ({ ...s, paymentTermsCode: e.target.value }))
                }
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-supplier-currency">
                {t("procurement.suppliers.fields.currency")}
              </Label>
              <Input
                id="inv-supplier-currency"
                className="font-mono uppercase"
                maxLength={3}
                value={supplierForm.currencyCode}
                placeholder={currencyCode}
                aria-describedby="inv-supplier-currency-hint"
                onChange={(e) =>
                  setSupplierForm((s) => ({ ...s, currencyCode: e.target.value.toUpperCase() }))
                }
              />
              <p id="inv-supplier-currency-hint" className="text-xs text-text-muted">
                {t("procurement.suppliers.fields.currencyHint", { currency: currencyCode })}
              </p>
            </div>
            <div className="flex items-center gap-2 pt-4">
              <input
                type="checkbox"
                id="supplier-active"
                className="h-4 w-4"
                checked={supplierForm.isActive}
                onChange={(e) => setSupplierForm((s) => ({ ...s, isActive: e.target.checked }))}
              />
              <Label htmlFor="supplier-active">{t("procurement.suppliers.fields.active")}</Label>
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setSupplierEditOpen(false)}>
              {t("common:action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={saving || !supplierForm.code.trim() || !supplierForm.name.trim()}
              onClick={() => void saveSupplierForm()}
            >
              {t("common:action.save")}
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
              <Label htmlFor="inv-price-article">
                {t("procurement.suppliers.prices.fields.article")}
              </Label>
              <Select
                value={String(priceFormArticleId)}
                onValueChange={(v) => setPriceFormArticleId(Number(v))}
              >
                <SelectTrigger id="inv-price-article">
                  <SelectValue placeholder={t("procurement.requisitionForm.selectArticle")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.requisitionForm.selectArticle")}
                  </SelectItem>
                  {articles.map((a) => (
                    <SelectItem key={a.id} value={String(a.id)}>
                      {a.article_code} — {a.article_name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-price-unit">
                {t("procurement.suppliers.prices.fields.unitPrice")}
              </Label>
              <Input
                id="inv-price-unit"
                type="number"
                min={0}
                step="0.01"
                value={priceFormUnitPrice}
                onChange={(e) => setPriceFormUnitPrice(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-price-min">
                {t("procurement.suppliers.prices.fields.minQty")}
              </Label>
              <Input
                id="inv-price-min"
                type="number"
                min={0}
                step="0.01"
                value={priceFormMinQty}
                onChange={(e) => setPriceFormMinQty(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-price-from">
                {t("procurement.suppliers.prices.fields.validFrom")}
              </Label>
              <Input
                id="inv-price-from"
                type="date"
                value={priceFormValidFrom}
                onChange={(e) => setPriceFormValidFrom(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setPriceFormOpen(false)}>
              {t("common:action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={saving || priceFormArticleId <= 0 || !priceFormUnitPrice}
              onClick={() => void savePriceForm()}
            >
              {t("common:action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ProcurementArchivePanel
        requisitions={requisitions}
        purchaseOrders={purchaseOrders}
        repairables={repairables}
        onRequisitionClick={(req) => {
          setActiveTab("requisitions");
          openRequisitionDetails(req.id);
        }}
        onPurchaseOrderClick={(po) => {
          setActiveTab("purchase-orders");
          openPoDetails(po.id);
        }}
        onRepairableClick={(order) => {
          setActiveTab("repairables");
          openRepairableDetails(order.id);
        }}
      />

      {/* Document link form dialog */}
      <Dialog open={docLinkFormOpen} onOpenChange={setDocLinkFormOpen}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("procurement.suppliers.documents.new")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="space-y-1">
              <Label htmlFor="inv-doc-ref">{t("procurement.suppliers.documents.fields.ref")}</Label>
              <Input
                id="inv-doc-ref"
                value={docLinkFormRef}
                onChange={(e) => setDocLinkFormRef(e.target.value)}
                placeholder={t("procurement.suppliers.documents.refPlaceholder")}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="inv-doc-purpose">
                {t("procurement.suppliers.documents.fields.purpose")}
              </Label>
              <Select value={docLinkFormPurpose} onValueChange={setDocLinkFormPurpose}>
                <SelectTrigger id="inv-doc-purpose">
                  <SelectValue placeholder={t("procurement.suppliers.documents.selectPurpose")} />
                </SelectTrigger>
                <SelectContent>
                  {documentPurposes.options.map((purpose) => (
                    <SelectItem key={purpose.id} value={purpose.code}>
                      {documentPurposes.labelOf(purpose.code)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {documentPurposes.error ? (
                <p className="text-xs text-text-muted">
                  {t("procurement.suppliers.documents.purposesUnavailable")}
                </p>
              ) : null}
            </div>
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                id="doc-link-primary"
                className="h-4 w-4"
                checked={docLinkFormPrimary}
                onChange={(e) => setDocLinkFormPrimary(e.target.checked)}
              />
              <Label htmlFor="doc-link-primary">
                {t("procurement.suppliers.documents.fields.primary")}
              </Label>
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setDocLinkFormOpen(false)}>
              {t("common:action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={saving || !docLinkFormRef.trim() || !docLinkFormPurpose}
              onClick={() => void saveDocLinkForm()}
            >
              {t("common:action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={pendingConfirm !== null}
        onOpenChange={(open) => {
          if (!open) setPendingConfirm(null);
        }}
        title={pendingConfirm?.title ?? ""}
        description={pendingConfirm?.description ?? ""}
        confirmLabel={pendingConfirm?.confirmLabel ?? ""}
        destructive
        onConfirm={async () => {
          if (!pendingConfirm) return;
          const ok = await pendingConfirm.run();
          // A rejected promise keeps the confirmation open (the toast explains the failure).
          if (!ok) throw new Error("action failed");
        }}
      />
    </div>
  );
});
