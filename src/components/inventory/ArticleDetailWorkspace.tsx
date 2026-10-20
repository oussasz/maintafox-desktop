import {
  AlertTriangle,
  ClipboardList,
  MoreHorizontal,
  Package,
  Pencil,
  Tag,
  Warehouse,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import {
  DetailFieldRow,
  DetailSectionCard,
  ENTITY_DETAIL_HEADER_ACTION_BTN,
  EntityActivityTimeline,
  EntityDetailHeader,
  EntityDetailViewSwitcher,
  EntityQrCode,
  EntitySummaryKpiStrip,
} from "@/components/detail";
import { ArticleConsumptionChart } from "@/components/inventory/ArticleConsumptionChart";
import { InventoryAdjustmentDialog } from "@/components/inventory/InventoryAdjustmentDialog";
import { ReservationInsightsPanel } from "@/components/inventory/ReservationInsightsPanel";
import { StockTransferAction } from "@/components/inventory/StockTransferAction";
import {
  type SourceSupplierOption,
  SupplierArticleSourcesSection,
} from "@/components/inventory/SupplierArticleSourcesSection";
import { SupplierComparisonCard } from "@/components/inventory/SupplierComparisonCard";
import {
  composeArticleActivity,
  computeArticleKpis,
} from "@/components/inventory/article-activity";
import { useLookupOptions } from "@/components/inventory/use-lookup-options";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
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
import { PROCUREMENT_STATUS_TONE, REPAIRABLE_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { formatOrDash, formatSiteLabel } from "@/lib/display";
import {
  type InventoryArticleRow,
  deleteInventoryArticleEquivalent,
  getInventoryArticleRepairableHistory,
  isBelowReorderPoint,
  listInventoryArticleEquivalents,
  listInventoryArticlePurchaseHistory,
  listInventoryDocumentLinks,
  listInventoryReservations,
  listInventoryStateEvents,
  listInventoryStockBalances,
  listInventorySuppliers,
  listInventoryTransactions,
  upsertInventoryArticleEquivalent,
  upsertInventoryDocumentLink,
} from "@/services/inventory-service";
import { useInventoryStore } from "@/stores/inventory-store";
import type { ArticleEquivalent } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type ArticleDetailView =
  | "details"
  | "activity"
  | "stock_positions"
  | "reservations"
  | "purchase_history"
  | "equivalents"
  | "repairable_history"
  | "documents";

export interface ArticleDetailWorkspaceProps {
  article: InventoryArticleRow;
  onEdit: () => void;
  onDeactivate: () => void;
  onCreateRequisition: () => void;
  /** Optional: refresh parent list after a stock change. */
  onStockAdjusted?: () => void;
  /** Optional: open a purchase order from the purchase history section. */
  onOpenPurchaseOrder?: (purchaseOrderId: number) => void;
}

/** One independently loaded section of the article detail. */
interface Section<T> {
  data: T;
  loading: boolean;
  error: unknown;
  reload: () => void;
}

/**
 * Loads one section on its own, so a failing call shows an ErrorState with retry in that
 * section only instead of blanking the whole article panel.
 */
function useSection<T>(load: () => Promise<T>, initial: T, key: unknown): Section<T> {
  const [data, setData] = useState<T>(initial);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    load()
      .then((value) => {
        if (!cancelled) setData(value);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
    // `load` closes over `key`; reloading on key/version change is the contract.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, version]);

  const reload = useCallback(() => setVersion((v) => v + 1), []);
  return { data, loading, error, reload };
}

/** Section body: loading, error with retry, or the content. */
function SectionBody({
  section,
  children,
}: {
  section: Pick<Section<unknown>, "loading" | "error" | "reload">;
  children: React.ReactNode;
}) {
  if (section.error) {
    return <ErrorState error={section.error} onRetry={section.reload} />;
  }
  if (section.loading) return <LoadingState className="py-6" />;
  return <>{children}</>;
}

export function ArticleDetailWorkspace({
  article,
  onEdit,
  onDeactivate,
  onCreateRequisition,
  onStockAdjusted,
  onOpenPurchaseOrder,
}: ArticleDetailWorkspaceProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const { formatDate, formatDateTime, formatNumber, formatCurrency } = useFormatters();
  const storeArticles = useInventoryStore((s) => s.articles);
  const equivalenceTypes = useLookupOptions("inventory.equivalence_type");
  const documentPurposes = useLookupOptions("inventory.document_link_purpose");
  const movementReasons = useLookupOptions("inventory.movement_type");
  const [activeView, setActiveView] = useState<ArticleDetailView>("details");
  const [adjustOpen, setAdjustOpen] = useState(false);

  const articleId = article.id;
  const balances = useSection(
    () => listInventoryStockBalances({ article_id: articleId, warehouse_id: null }),
    [],
    articleId,
  );
  const transactions = useSection(
    () => listInventoryTransactions({ article_id: articleId, limit: 200 }),
    [],
    articleId,
  );
  const reservations = useSection(
    () => listInventoryReservations({ article_id: articleId, include_inactive: true }),
    [],
    articleId,
  );
  const stateEvents = useSection(
    () => listInventoryStateEvents("ARTICLE", articleId),
    [],
    articleId,
  );
  const purchaseHistory = useSection(
    () => listInventoryArticlePurchaseHistory(articleId),
    [],
    articleId,
  );
  const equivalents = useSection(() => listInventoryArticleEquivalents(articleId), [], articleId);
  const repairableHistory = useSection(
    () => getInventoryArticleRepairableHistory(articleId),
    [],
    articleId,
  );
  const documentLinks = useSection(
    () => listInventoryDocumentLinks("ARTICLE", articleId),
    [],
    articleId,
  );
  const suppliers = useSection<SourceSupplierOption[]>(
    () =>
      listInventorySuppliers().then((rows) =>
        // Blocked suppliers are not valid sourcing candidates.
        rows
          .filter((s) => s.is_active === 1 && s.status_code !== "BLOCKED")
          .map((s) => ({ id: s.id, code: s.code, name: s.name })),
      ),
    [],
    articleId,
  );
  const [sourcingVersion, setSourcingVersion] = useState(0);

  // Equivalent form
  const [equivFormOpen, setEquivFormOpen] = useState(false);
  const [equivArticleId, setEquivArticleId] = useState<number>(0);
  const [equivType, setEquivType] = useState<string>("");
  const [equivNotes, setEquivNotes] = useState<string>("");
  const [equivBidirectional, setEquivBidirectional] = useState<boolean>(true);
  const [equivSaving, setEquivSaving] = useState(false);
  const [equivToRemove, setEquivToRemove] = useState<ArticleEquivalent | null>(null);

  // Document link form
  const [docFormOpen, setDocFormOpen] = useState(false);
  const [docRef, setDocRef] = useState<string>("");
  const [docPurpose, setDocPurpose] = useState<string>("");
  const [docPrimary, setDocPrimary] = useState<boolean>(false);
  const [docSaving, setDocSaving] = useState(false);

  const otherActiveArticles = useMemo(
    () => storeArticles.filter((a) => a.is_active === 1 && a.id !== articleId),
    [storeArticles, articleId],
  );

  const formatQty = useCallback(
    (n: number) => formatNumber(n, { maximumFractionDigits: 3 }),
    [formatNumber],
  );
  const dateLabel = useCallback(
    (v: string | null | undefined) => (v ? formatDate(v) : "—"),
    [formatDate],
  );

  const handleStockAdjusted = useCallback(() => {
    balances.reload();
    transactions.reload();
    onStockAdjusted?.();
  }, [balances, transactions, onStockAdjusted]);

  /** Keeps the sources list and the comparison card in sync after either mutates. */
  const handleSourcesChanged = useCallback(() => {
    setSourcingVersion((v) => v + 1);
  }, []);

  const kpis = useMemo(
    () => computeArticleKpis(balances.data, transactions.data),
    [balances.data, transactions.data],
  );

  const activityItems = useMemo(
    () =>
      composeArticleActivity({
        article,
        stateEvents: stateEvents.data,
        transactions: transactions.data,
        reservations: reservations.data,
        labels: {
          motif: (code) => movementReasons.labelOf(code),
          reservationStatus: (code) =>
            t(`detail.reservationStatuses.${code}`, { defaultValue: code }),
          qty: formatQty,
        },
      }),
    [
      article,
      formatQty,
      movementReasons,
      reservations.data,
      stateEvents.data,
      t,
      transactions.data,
    ],
  );

  const tabs = useMemo(
    () => [
      { id: "details", label: t("detail.tabs.details") },
      { id: "activity", label: t("detail.tabs.activity") },
      { id: "stock_positions", label: t("detail.tabs.stockPositions") },
      { id: "reservations", label: t("detail.tabs.reservations") },
      { id: "purchase_history", label: t("article.sections.purchaseHistory") },
      { id: "equivalents", label: t("article.sections.equivalents") },
      { id: "repairable_history", label: t("article.sections.repairableHistory") },
      { id: "documents", label: t("article.sections.documents") },
    ],
    [t],
  );

  const saveEquivalent = async () => {
    if (equivArticleId <= 0 || !equivType) return;
    setEquivSaving(true);
    try {
      await upsertInventoryArticleEquivalent({
        article_id: article.id,
        equivalent_article_id: equivArticleId,
        equivalence_type: equivType,
        notes: equivNotes.trim() || null,
        is_bidirectional: equivBidirectional,
      });
      equivalents.reload();
      setEquivFormOpen(false);
      setEquivArticleId(0);
      setEquivNotes("");
    } catch {
      // Reported by the global IPC toast; the dialog stays open with the user's input.
    } finally {
      setEquivSaving(false);
    }
  };

  const removeEquivalent = async () => {
    if (!equivToRemove) return;
    // Rejects on failure so ConfirmDialog stays open (the toast explains why).
    await deleteInventoryArticleEquivalent(equivToRemove.id);
    equivalents.reload();
  };

  const saveDocLink = async () => {
    if (!docRef.trim() || !docPurpose) return;
    setDocSaving(true);
    try {
      await upsertInventoryDocumentLink({
        entity_type: "ARTICLE",
        entity_id: article.id,
        document_ref: docRef.trim(),
        link_purpose: docPurpose,
        is_primary: docPrimary,
      });
      documentLinks.reload();
      setDocFormOpen(false);
      setDocRef("");
    } catch {
      // Reported by the global IPC toast; the dialog stays open with the user's input.
    } finally {
      setDocSaving(false);
    }
  };

  const activitySection = {
    loading: stateEvents.loading || transactions.loading || reservations.loading,
    error: stateEvents.error ?? transactions.error ?? reservations.error,
    reload: () => {
      if (stateEvents.error) stateEvents.reload();
      if (transactions.error) transactions.reload();
      if (reservations.error) reservations.reload();
    },
  };

  const lowStock = isBelowReorderPoint(article);

  return (
    <div className="flex h-full flex-col space-y-4 overflow-auto p-4">
      <EntityDetailHeader
        code={article.article_code}
        designation={article.article_name}
        statusSlot={
          <div className="flex flex-wrap items-center gap-1.5">
            <StatusChip
              label={article.is_active === 1 ? tc("status.active") : tc("status.inactive")}
              tone={article.is_active === 1 ? "success" : "neutral"}
              dot
            />
            {lowStock ? (
              <StatusChip
                label={t("list.belowReorder")}
                tone="warning"
                icon={AlertTriangle}
                title={t("list.belowReorderHint", {
                  reorderPoint: formatQty(article.reorder_point),
                })}
              />
            ) : null}
          </div>
        }
        qrSlot={
          <EntityQrCode
            code={article.article_code}
            name={article.article_name}
            payload={`maintafox://article/${article.id}`}
            viewPermission={P.INV_VIEW}
            metadata={[
              {
                label: t("detail.fields.unit"),
                value: article.unit_label || article.unit_code || "—",
              },
            ]}
            dialogTitle={t("detail.qr.dialogTitle")}
            scanHint={t("detail.qr.scanHint")}
          />
        }
        primaryActionsSlot={
          <>
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                variant="default"
                size="sm"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={() => setAdjustOpen(true)}
                disabled={article.is_active !== 1}
                title={article.is_active !== 1 ? t("detail.inactiveHint") : undefined}
              >
                <Package className="h-3.5 w-3.5" aria-hidden />
                {t("detail.actions.adjustStock")}
              </Button>
            </PermissionGate>
            <PermissionGate permission={P.INV_PROCURE}>
              <Button
                variant="outline"
                size="sm"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={onCreateRequisition}
                disabled={article.is_active !== 1}
                title={article.is_active !== 1 ? t("detail.inactiveHint") : undefined}
              >
                <ClipboardList className="h-3.5 w-3.5" aria-hidden />
                {t("detail.actions.createRequisition")}
              </Button>
            </PermissionGate>
          </>
        }
        overflowSlot={
          <PermissionGate permission={P.INV_MANAGE}>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-10 w-10 px-0"
                  aria-label={t("detail.actions.more")}
                >
                  <MoreHorizontal className="h-4 w-4" aria-hidden />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-52">
                <DropdownMenuItem className="gap-2" onClick={onEdit}>
                  <Pencil className="h-3.5 w-3.5" aria-hidden />
                  {tc("action.edit")}
                </DropdownMenuItem>
                {article.is_active === 1 ? (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      className="gap-2 text-status-danger focus:text-status-danger"
                      onClick={onDeactivate}
                    >
                      <AlertTriangle className="h-3.5 w-3.5" aria-hidden />
                      {t("detail.actions.deactivate")}
                    </DropdownMenuItem>
                  </>
                ) : null}
              </DropdownMenuContent>
            </DropdownMenu>
          </PermissionGate>
        }
        footerSlot={
          // List totals come with the article row; locations and 30-day movements need the
          // balance / movement sections.
          <EntitySummaryKpiStrip
            title={t("detail.kpis.title")}
            kpis={[
              {
                id: "onHand",
                label: t("detail.kpis.onHand"),
                value: `${formatQty(article.on_hand_qty)} ${article.unit_code}`,
              },
              {
                id: "reserved",
                label: t("detail.kpis.reserved"),
                value: formatQty(article.reserved_qty),
              },
              {
                id: "available",
                label: t("detail.kpis.available"),
                value: formatQty(article.available_qty),
              },
              {
                id: "avgCost",
                label: t("detail.kpis.avgCost"),
                value: article.avg_unit_cost != null ? formatCurrency(article.avg_unit_cost) : "—",
              },
              {
                id: "stockValue",
                label: t("detail.kpis.stockValue"),
                value: article.stock_value != null ? formatCurrency(article.stock_value) : "—",
              },
              {
                id: "locations",
                label: t("detail.kpis.locations"),
                value: balances.loading || balances.error ? "—" : formatNumber(kpis.locations),
              },
              {
                id: "movements30d",
                label: t("detail.kpis.movements30d"),
                value:
                  transactions.loading || transactions.error
                    ? "—"
                    : formatNumber(kpis.movements30d),
              },
            ]}
          />
        }
      />

      <EntityDetailViewSwitcher
        tabs={tabs}
        activeId={activeView}
        onChange={(id) => setActiveView(id as ArticleDetailView)}
      />

      {/* ── ACTIVITY ── */}
      {activeView === "activity" ? (
        <Card>
          <CardContent className="pt-4">
            <SectionBody section={activitySection}>
              <EntityActivityTimeline
                items={activityItems}
                emptyLabel={t("detail.activity.empty")}
                todayLabel={t("detail.activity.today")}
                searchPlaceholder={t("detail.activity.search")}
                kindAllLabel={t("detail.activity.kindAll")}
                periodAllLabel={t("detail.activity.periodAll")}
                period30dLabel={t("detail.activity.period30d")}
                period90dLabel={t("detail.activity.period90d")}
                typeLabel={(key) =>
                  t(`detail.activity.types.${key}`, {
                    defaultValue: t("detail.activity.types.other"),
                  })
                }
                kindFilterOptions={[
                  { value: "all", label: t("detail.activity.kindAll") },
                  { value: "created", label: t("detail.activity.kinds.created") },
                  { value: "edited", label: t("detail.activity.kinds.edited") },
                  { value: "stock_movement", label: t("detail.activity.kinds.stock_movement") },
                  {
                    value: "reservation_created",
                    label: t("detail.activity.kinds.reservation_created"),
                  },
                  {
                    value: "reservation_released",
                    label: t("detail.activity.kinds.reservation_released"),
                  },
                ]}
              />
            </SectionBody>
          </CardContent>
        </Card>
      ) : null}

      {/* ── STOCK POSITIONS ── */}
      {activeView === "stock_positions" ? (
        <Card>
          <CardContent className="space-y-2 pt-4">
            <SectionBody section={balances}>
              {balances.data.length === 0 ? (
                <p className="text-sm text-text-muted">{t("detail.stockPositions.empty")}</p>
              ) : (
                balances.data.map((b) => (
                  <div key={b.id} className="rounded border border-surface-border p-2.5 text-sm">
                    <div className="flex items-start justify-between gap-2">
                      <div className="font-medium">
                        {formatSiteLabel(b.warehouse_code, b.warehouse_name)}
                        <span className="text-text-muted"> / </span>
                        {formatSiteLabel(b.location_code, b.location_name)}
                      </div>
                      <StockTransferAction
                        article={article}
                        balance={b}
                        onTransferred={handleStockAdjusted}
                      />
                    </div>
                    <div className="mt-1 text-xs text-text-muted">
                      {t("detail.kpis.onHand")}: {formatQty(b.on_hand_qty)} ·{" "}
                      {t("detail.kpis.reserved")}: {formatQty(b.reserved_qty)} ·{" "}
                      {t("detail.kpis.available")}: {formatQty(b.available_qty)} ·{" "}
                      {t("detail.kpis.avgCost")}:{" "}
                      {b.moving_avg_unit_cost != null
                        ? formatCurrency(b.moving_avg_unit_cost)
                        : t("detail.stockPositions.notValued")}
                    </div>
                  </div>
                ))
              )}
            </SectionBody>
          </CardContent>
        </Card>
      ) : null}

      {/* ── RESERVATIONS ── */}
      {activeView === "reservations" ? (
        <Card>
          <CardContent className="space-y-4 pt-4">
            <SectionBody section={reservations}>
              <ReservationInsightsPanel reservations={reservations.data} />
            </SectionBody>
            <ArticleConsumptionChart articleId={article.id} />
          </CardContent>
        </Card>
      ) : null}

      {/* ── PURCHASE HISTORY ── */}
      {activeView === "purchase_history" ? (
        <Card>
          <CardContent className="pt-4">
            <SectionBody section={purchaseHistory}>
              {purchaseHistory.data.length === 0 ? (
                <p className="text-sm text-text-muted">{t("article.purchaseHistory.empty")}</p>
              ) : (
                <div className="space-y-2">
                  {purchaseHistory.data.map((row) => (
                    <div
                      key={row.purchase_order_id}
                      className="rounded border border-surface-border p-2.5 text-sm"
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        {onOpenPurchaseOrder ? (
                          <button
                            type="button"
                            className="min-h-6 font-mono font-medium text-primary hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            onClick={() => onOpenPurchaseOrder(row.purchase_order_id)}
                          >
                            {row.po_number}
                          </button>
                        ) : (
                          <span className="font-mono font-medium">{row.po_number}</span>
                        )}
                        <StatusChip
                          label={t(`procurement.statuses.${row.status}`, {
                            defaultValue: row.status,
                          })}
                          tone={toneOf(PROCUREMENT_STATUS_TONE, row.status)}
                          dot
                        />
                        {row.supplier_name ? (
                          <span className="text-xs text-text-muted">{row.supplier_name}</span>
                        ) : null}
                      </div>
                      <div className="mt-1 flex flex-wrap gap-4 text-xs text-text-muted">
                        <span>
                          {t("article.purchaseHistory.columns.orderedQty")}:{" "}
                          {formatQty(row.ordered_qty)}
                        </span>
                        <span>
                          {t("article.purchaseHistory.columns.receivedQty")}:{" "}
                          {formatQty(row.received_qty)}
                        </span>
                        {row.unit_price != null ? (
                          <span>
                            {t("article.purchaseHistory.columns.unitPrice")}:{" "}
                            {formatCurrency(row.unit_price)}
                          </span>
                        ) : null}
                        {row.ordered_at ? <span>{dateLabel(row.ordered_at)}</span> : null}
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </SectionBody>
          </CardContent>
        </Card>
      ) : null}

      {/* ── EQUIVALENTS ── */}
      {activeView === "equivalents" ? (
        <Card>
          <CardContent className="pt-4">
            <div className="mb-3 flex items-center justify-between">
              <p className="text-sm font-medium">{t("article.sections.equivalents")}</p>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button
                  variant="outline"
                  className="h-10"
                  onClick={() => {
                    setEquivArticleId(0);
                    setEquivType(equivalenceTypes.options[0]?.code ?? "");
                    setEquivNotes("");
                    setEquivBidirectional(true);
                    setEquivFormOpen(true);
                  }}
                >
                  {t("article.actions.addEquivalent")}
                </Button>
              </PermissionGate>
            </div>
            <SectionBody section={equivalents}>
              {equivalents.data.length === 0 ? (
                <p className="text-sm text-text-muted">{t("article.equivalents.empty")}</p>
              ) : (
                <div className="space-y-2">
                  {equivalents.data.map((eq) => (
                    <div
                      key={eq.id}
                      className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-2.5 text-sm"
                    >
                      <span className="font-medium">
                        {eq.equivalent_code} — {eq.equivalent_name}
                      </span>
                      <Badge variant="outline" className="text-[10px]">
                        {equivalenceTypes.labelOf(eq.equivalence_type)}
                      </Badge>
                      {eq.is_bidirectional === 1 ? (
                        <span className="text-xs text-text-muted">
                          {t("article.equivalents.fields.bidirectional")}
                        </span>
                      ) : null}
                      {eq.notes ? (
                        <span className="text-xs text-text-muted">{eq.notes}</span>
                      ) : null}
                      <PermissionGate permission={P.INV_MANAGE}>
                        <Button
                          variant="ghost"
                          className="ml-auto h-9 px-2 text-xs text-status-danger hover:text-status-danger"
                          onClick={() => setEquivToRemove(eq)}
                        >
                          {t("article.equivalents.remove")}
                        </Button>
                      </PermissionGate>
                    </div>
                  ))}
                </div>
              )}
            </SectionBody>
          </CardContent>
        </Card>
      ) : null}

      {/* ── REPAIRABLE HISTORY ── */}
      {activeView === "repairable_history" ? (
        <Card>
          <CardContent className="pt-4">
            <SectionBody section={repairableHistory}>
              {repairableHistory.data.length === 0 ? (
                <p className="text-sm text-text-muted">{t("article.repairableHistory.empty")}</p>
              ) : (
                <div className="space-y-2">
                  {repairableHistory.data.map((row) => (
                    <div
                      key={row.order_id}
                      className="rounded border border-surface-border p-2.5 text-sm"
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-mono font-medium">{row.order_code}</span>
                        <StatusChip
                          label={t(`procurement.repairableStatuses.${row.status}`, {
                            defaultValue: row.status,
                          })}
                          tone={toneOf(REPAIRABLE_STATUS_TONE, row.status)}
                          dot
                        />
                        <span className="tabular-nums">×{formatQty(row.quantity)}</span>
                      </div>
                      <div className="mt-1 flex flex-wrap gap-4 text-xs text-text-muted">
                        {row.reason ? <span>{row.reason}</span> : null}
                        {row.repair_cost != null ? (
                          <span>
                            {t("article.repairableHistory.columns.cost")}:{" "}
                            {formatCurrency(row.repair_cost)}
                          </span>
                        ) : null}
                        <span>{dateLabel(row.created_at)}</span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </SectionBody>
          </CardContent>
        </Card>
      ) : null}

      {/* ── DOCUMENTS ── */}
      {activeView === "documents" ? (
        <Card>
          <CardContent className="pt-4">
            <div className="mb-3 flex items-center justify-between">
              <p className="text-sm font-medium">{t("article.sections.documents")}</p>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button
                  variant="outline"
                  className="h-10"
                  onClick={() => {
                    setDocRef("");
                    setDocPurpose(documentPurposes.options[0]?.code ?? "");
                    setDocPrimary(false);
                    setDocFormOpen(true);
                  }}
                >
                  {t("article.actions.addDocumentLink")}
                </Button>
              </PermissionGate>
            </div>
            <SectionBody section={documentLinks}>
              {documentLinks.data.length === 0 ? (
                <p className="text-sm text-text-muted">{t("article.documents.empty")}</p>
              ) : (
                <div className="space-y-2">
                  {documentLinks.data.map((doc) => (
                    <div
                      key={doc.id}
                      className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-2.5 text-sm"
                    >
                      <span className="break-all font-medium">{doc.document_ref}</span>
                      <Badge variant="outline" className="text-[10px]">
                        {documentPurposes.labelOf(doc.link_purpose)}
                      </Badge>
                      {doc.is_primary === 1 ? (
                        <Badge variant="secondary" className="text-[10px]">
                          {t("article.documents.columns.primary")}
                        </Badge>
                      ) : null}
                      <span className="text-xs text-text-muted">{dateLabel(doc.valid_from)}</span>
                    </div>
                  ))}
                </div>
              )}
            </SectionBody>
          </CardContent>
        </Card>
      ) : null}

      {/* ── DETAILS ── */}
      {activeView === "details" ? (
        <>
          <DetailSectionCard title={t("detail.sections.classification")} icon={Tag}>
            <DetailFieldRow
              label={t("detail.fields.family")}
              value={formatOrDash(article.family_name ?? article.family_code)}
            />
            <DetailFieldRow
              label={t("detail.fields.stockingType")}
              value={formatOrDash(article.stocking_type_label || article.stocking_type_code)}
            />
            <DetailFieldRow
              label={t("detail.fields.procurementCategory")}
              value={formatOrDash(
                article.procurement_category_label ?? article.procurement_category_code,
              )}
            />
            <DetailFieldRow
              label={t("detail.fields.criticality")}
              value={formatOrDash(article.criticality_label ?? article.criticality_code)}
            />
            <DetailFieldRow
              label={t("article.fields.abcClass")}
              value={formatOrDash(article.abc_class_code)}
            />
            <DetailFieldRow
              label={t("article.fields.xyzClass")}
              value={formatOrDash(article.xyz_class_code)}
            />
          </DetailSectionCard>

          <DetailSectionCard title={t("detail.sections.identification")} icon={Tag}>
            <DetailFieldRow label={t("detail.fields.code")} value={article.article_code} mono />
            <DetailFieldRow label={t("detail.fields.name")} value={article.article_name} />
            <DetailFieldRow
              label={t("detail.fields.unit")}
              value={formatOrDash(article.unit_label || article.unit_code)}
            />
            <DetailFieldRow
              label={t("detail.fields.taxCategory")}
              value={formatOrDash(article.tax_category_label || article.tax_category_code)}
            />
            <DetailFieldRow
              label={t("article.fields.manufacturerName")}
              value={formatOrDash(article.manufacturer_name)}
            />
            <DetailFieldRow
              label={t("article.fields.manufacturerPartNumber")}
              value={formatOrDash(article.manufacturer_part_number)}
              mono
            />
            <DetailFieldRow
              label={tc("label.status")}
              value={article.is_active === 1 ? tc("status.active") : tc("status.inactive")}
            />
          </DetailSectionCard>

          <DetailSectionCard title={t("detail.sections.inventory")} icon={Package}>
            <DetailFieldRow
              label={t("detail.fields.minStock")}
              value={formatQty(article.min_stock)}
            />
            <DetailFieldRow
              label={t("detail.fields.maxStock")}
              value={article.max_stock != null ? formatQty(article.max_stock) : "—"}
            />
            <DetailFieldRow
              label={t("detail.fields.reorderPoint")}
              value={formatQty(article.reorder_point)}
            />
            <DetailFieldRow
              label={t("detail.fields.safetyStock")}
              value={formatQty(article.safety_stock)}
            />
            <DetailFieldRow
              label={t("article.fields.leadTimeDays")}
              value={article.lead_time_days != null ? formatNumber(article.lead_time_days) : "—"}
            />
            <DetailFieldRow
              label={t("article.fields.isCriticalSpare")}
              value={article.is_critical_spare === 1 ? tc("action.yes") : tc("action.no")}
            />
          </DetailSectionCard>

          <DetailSectionCard title={t("detail.sections.warehouse")} icon={Warehouse}>
            <DetailFieldRow
              label={t("detail.fields.preferredWarehouse")}
              value={formatSiteLabel(
                article.preferred_warehouse_code,
                article.preferred_warehouse_name,
              )}
            />
            <DetailFieldRow
              label={t("detail.fields.preferredLocation")}
              value={formatSiteLabel(
                article.preferred_location_code,
                article.preferred_location_name,
              )}
            />
          </DetailSectionCard>

          <DetailSectionCard title={t("detail.sections.lifecycle")} icon={Tag}>
            <DetailFieldRow
              label={tc("label.createdAt")}
              value={article.created_at ? formatDateTime(article.created_at) : "—"}
            />
            <DetailFieldRow
              label={tc("label.updatedAt")}
              value={article.updated_at ? formatDateTime(article.updated_at) : "—"}
            />
          </DetailSectionCard>

          {suppliers.error ? (
            <ErrorState
              error={suppliers.error}
              title={t("detail.suppliersLoadError")}
              onRetry={suppliers.reload}
            />
          ) : (
            <SupplierArticleSourcesSection
              articleId={article.id}
              suppliers={suppliers.data}
              onChanged={handleSourcesChanged}
              refreshToken={sourcingVersion}
            />
          )}

          <SupplierComparisonCard
            articleId={article.id}
            onPreferredChanged={handleSourcesChanged}
            refreshToken={sourcingVersion}
          />
        </>
      ) : null}

      {/* ── ADJUSTMENT DIALOG ── */}
      <InventoryAdjustmentDialog
        articleId={article.id}
        defaultWarehouseId={article.preferred_warehouse_id}
        defaultLocationId={article.preferred_location_id}
        open={adjustOpen}
        onOpenChange={setAdjustOpen}
        onSuccess={handleStockAdjusted}
      />

      {/* ── EQUIVALENT FORM DIALOG ── */}
      <Dialog open={equivFormOpen} onOpenChange={setEquivFormOpen}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("article.actions.addEquivalent")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="space-y-1">
              <Label htmlFor="equiv-article">
                {t("article.equivalents.fields.equivalentArticle")}
              </Label>
              <Select
                value={String(equivArticleId)}
                onValueChange={(v) => setEquivArticleId(Number(v))}
              >
                <SelectTrigger id="equiv-article">
                  <SelectValue placeholder={t("procurement.requisitionForm.selectArticle")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.requisitionForm.selectArticle")}
                  </SelectItem>
                  {otherActiveArticles.map((a) => (
                    <SelectItem key={a.id} value={String(a.id)}>
                      {a.article_code} — {a.article_name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="equiv-type">{t("article.equivalents.fields.type")}</Label>
              <Select value={equivType} onValueChange={setEquivType}>
                <SelectTrigger id="equiv-type">
                  <SelectValue placeholder={t("article.equivalents.selectType")} />
                </SelectTrigger>
                <SelectContent>
                  {equivalenceTypes.options.map((option) => (
                    <SelectItem key={option.id} value={option.code}>
                      {equivalenceTypes.labelOf(option.code)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {equivalenceTypes.error ? (
                <p className="text-xs text-text-muted">
                  {t("article.equivalents.typesUnavailable")}
                </p>
              ) : null}
            </div>
            <div className="flex items-center gap-2">
              <input
                id="equiv-bidirectional"
                type="checkbox"
                className="h-4 w-4"
                checked={equivBidirectional}
                onChange={(e) => setEquivBidirectional(e.target.checked)}
              />
              <Label htmlFor="equiv-bidirectional">
                {t("article.equivalents.fields.bidirectional")}
              </Label>
            </div>
            <div className="space-y-1">
              <Label htmlFor="equiv-notes">{t("article.equivalents.fields.notes")}</Label>
              <Input
                id="equiv-notes"
                value={equivNotes}
                onChange={(e) => setEquivNotes(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setEquivFormOpen(false)}>
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={equivSaving || equivArticleId <= 0 || !equivType}
              onClick={() => void saveEquivalent()}
            >
              {tc("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={equivToRemove !== null}
        onOpenChange={(open) => {
          if (!open) setEquivToRemove(null);
        }}
        title={t("article.equivalents.confirmRemove.title")}
        description={t("article.equivalents.confirmRemove.description", {
          code: equivToRemove?.equivalent_code ?? "",
        })}
        confirmLabel={t("article.equivalents.confirmRemove.confirm")}
        destructive
        onConfirm={removeEquivalent}
      />

      {/* ── DOCUMENT LINK FORM DIALOG ── */}
      <Dialog open={docFormOpen} onOpenChange={setDocFormOpen}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("article.actions.addDocumentLink")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="space-y-1">
              <Label htmlFor="doc-ref">{t("procurement.suppliers.documents.fields.ref")}</Label>
              <Input
                id="doc-ref"
                value={docRef}
                onChange={(e) => setDocRef(e.target.value)}
                placeholder={t("procurement.suppliers.documents.refPlaceholder")}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="doc-purpose">
                {t("procurement.suppliers.documents.fields.purpose")}
              </Label>
              <Select value={docPurpose} onValueChange={setDocPurpose}>
                <SelectTrigger id="doc-purpose">
                  <SelectValue placeholder={t("procurement.suppliers.documents.selectPurpose")} />
                </SelectTrigger>
                <SelectContent>
                  {documentPurposes.options.map((option) => (
                    <SelectItem key={option.id} value={option.code}>
                      {documentPurposes.labelOf(option.code)}
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
                id="doc-primary"
                type="checkbox"
                className="h-4 w-4"
                checked={docPrimary}
                onChange={(e) => setDocPrimary(e.target.checked)}
              />
              <Label htmlFor="doc-primary">
                {t("procurement.suppliers.documents.fields.primary")}
              </Label>
            </div>
          </div>
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setDocFormOpen(false)}>
              {tc("action.cancel")}
            </Button>
            <Button
              className="h-11"
              disabled={docSaving || !docRef.trim() || !docPurpose}
              onClick={() => void saveDocLink()}
            >
              {tc("action.save")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
