import {
  AlertTriangle,
  ClipboardList,
  FileText,
  Loader2,
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
import { InventoryAdjustmentDialog } from "@/components/inventory/InventoryAdjustmentDialog";
import { ArticleConsumptionChart } from "@/components/inventory/ArticleConsumptionChart";
import { ReservationInsightsPanel } from "@/components/inventory/ReservationInsightsPanel";
import {
  SupplierArticleSourcesSection,
  type SourceSupplierOption,
} from "@/components/inventory/SupplierArticleSourcesSection";
import { SupplierComparisonCard } from "@/components/inventory/SupplierComparisonCard";
import {
  composeArticleActivity,
  computeArticleKpis,
} from "@/components/inventory/article-activity";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
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
import { formatOrDash, formatSiteLabel } from "@/lib/display";
import {
  deleteInventoryArticleEquivalent,
  getInventoryArticleRepairableHistory,
  listInventoryArticleEquivalents,
  listInventoryArticlePurchaseHistory,
  listInventoryArticles,
  listInventoryDocumentLinks,
  listInventoryReservations,
  listInventoryStateEvents,
  listInventoryStockBalances,
  listInventorySuppliers,
  listInventoryTransactions,
  upsertInventoryArticleEquivalent,
  upsertInventoryDocumentLink,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type {
  ArticleEquivalent,
  ArticlePurchaseHistoryRow,
  ArticleRepairableHistory,
  InventoryArticle,
  InventoryDocumentLink,
  InventoryStateEvent,
  InventoryStockBalance,
  InventoryTransaction,
  StockReservation,
} from "@shared/ipc-types";

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
  article: InventoryArticle;
  onEdit: () => void;
  onDeactivate: () => void;
  onCreateRequisition: () => void;
  /** Optional: refresh parent list after a stock change. */
  onStockAdjusted?: () => void;
}

function formatQty(n: number): string {
  return Number.isFinite(n) ? String(n) : "—";
}

function formatDate(v: string | null | undefined): string {
  if (!v) return "—";
  try {
    return new Date(v).toLocaleDateString();
  } catch {
    return v;
  }
}

const EQUIVALENCE_TYPES = ["SUBSTITUTE", "ALTERNATIVE", "INTERCHANGEABLE", "COMPATIBLE"] as const;

const DOC_LINK_PURPOSES = ["CONTRACT", "CERTIFICATE", "DATASHEET", "MANUAL", "OTHER"] as const;

export function ArticleDetailWorkspace({
  article,
  onEdit,
  onDeactivate,
  onCreateRequisition,
  onStockAdjusted,
}: ArticleDetailWorkspaceProps) {
  const { t } = useTranslation("inventory");
  const { t: tc } = useTranslation("common");
  const [activeView, setActiveView] = useState<ArticleDetailView>("details");
  const [adjustOpen, setAdjustOpen] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [balances, setBalances] = useState<InventoryStockBalance[]>([]);
  const [transactions, setTransactions] = useState<InventoryTransaction[]>([]);
  const [reservations, setReservations] = useState<StockReservation[]>([]);
  const [stateEvents, setStateEvents] = useState<InventoryStateEvent[]>([]);
  const [purchaseHistory, setPurchaseHistory] = useState<ArticlePurchaseHistoryRow[]>([]);
  const [equivalents, setEquivalents] = useState<ArticleEquivalent[]>([]);
  const [repairableHistory, setRepairableHistory] = useState<ArticleRepairableHistory[]>([]);
  const [documentLinks, setDocumentLinks] = useState<InventoryDocumentLink[]>([]);
  const [allArticles, setAllArticles] = useState<InventoryArticle[]>([]);
  const [sourcingSuppliers, setSourcingSuppliers] = useState<SourceSupplierOption[]>([]);
  const [sourcingVersion, setSourcingVersion] = useState(0);

  // Equivalent form
  const [equivFormOpen, setEquivFormOpen] = useState(false);
  const [equivArticleId, setEquivArticleId] = useState<number>(0);
  const [equivType, setEquivType] = useState<string>("SUBSTITUTE");
  const [equivNotes, setEquivNotes] = useState<string>("");
  const [equivBidirectional, setEquivBidirectional] = useState<boolean>(true);
  const [equivSaving, setEquivSaving] = useState(false);

  // Document link form
  const [docFormOpen, setDocFormOpen] = useState(false);
  const [docRef, setDocRef] = useState<string>("");
  const [docPurpose, setDocPurpose] = useState<string>("DATASHEET");
  const [docPrimary, setDocPrimary] = useState<boolean>(false);
  const [docSaving, setDocSaving] = useState(false);

  const load = useCallback(async (articleId: number) => {
    setLoading(true);
    setError(null);
    try {
      const [bal, tx, res, ev, history, equivs, repHistory, docs, articles, suppliers] =
        await Promise.all([
          listInventoryStockBalances({ article_id: articleId, warehouse_id: null }),
          listInventoryTransactions({ article_id: articleId, limit: 200 }),
          listInventoryReservations({ article_id: articleId, include_inactive: true }),
          listInventoryStateEvents("ARTICLE", articleId),
          listInventoryArticlePurchaseHistory(articleId),
          listInventoryArticleEquivalents(articleId),
          getInventoryArticleRepairableHistory(articleId),
          listInventoryDocumentLinks("ARTICLE", articleId),
          listInventoryArticles({ search: null }),
          listInventorySuppliers(),
        ]);
      setBalances(bal);
      setTransactions(tx);
      setReservations(res);
      setStateEvents(ev);
      setPurchaseHistory(history);
      setEquivalents(equivs);
      setRepairableHistory(repHistory);
      setDocumentLinks(docs);
      setAllArticles(articles.filter((a) => a.is_active === 1 && a.id !== articleId));
      // Blocked suppliers are not valid sourcing candidates.
      setSourcingSuppliers(
        suppliers
          .filter((s) => s.is_active === 1 && s.status_code !== "BLOCKED")
          .map((s) => ({ id: s.id, code: s.code, name: s.name })),
      );
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load(article.id);
  }, [article.id, load]);

  const handleStockAdjusted = useCallback(() => {
    void load(article.id);
    onStockAdjusted?.();
  }, [article.id, load, onStockAdjusted]);

  /** Keeps the sources list and the comparison card in sync after either mutates. */
  const handleSourcesChanged = useCallback(() => {
    setSourcingVersion((v) => v + 1);
  }, []);

  const kpis = useMemo(() => computeArticleKpis(balances, transactions), [balances, transactions]);

  const activityItems = useMemo(
    () => composeArticleActivity({ article, stateEvents, transactions, reservations }),
    [article, stateEvents, transactions, reservations],
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
    if (equivArticleId <= 0) return;
    setEquivSaving(true);
    setError(null);
    try {
      await upsertInventoryArticleEquivalent({
        article_id: article.id,
        equivalent_article_id: equivArticleId,
        equivalence_type: equivType,
        notes: equivNotes.trim() || null,
        is_bidirectional: equivBidirectional,
      });
      const updated = await listInventoryArticleEquivalents(article.id);
      setEquivalents(updated);
      setEquivFormOpen(false);
      setEquivArticleId(0);
      setEquivNotes("");
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setEquivSaving(false);
    }
  };

  const deleteEquivalent = async (id: number) => {
    setError(null);
    try {
      await deleteInventoryArticleEquivalent(id);
      setEquivalents((prev) => prev.filter((e) => e.id !== id));
    } catch (err) {
      setError(toErrorMessage(err));
    }
  };

  const saveDocLink = async () => {
    if (!docRef.trim()) return;
    setDocSaving(true);
    setError(null);
    try {
      await upsertInventoryDocumentLink({
        entity_type: "ARTICLE",
        entity_id: article.id,
        document_ref: docRef.trim(),
        link_purpose: docPurpose,
        is_primary: docPrimary,
      });
      const updated = await listInventoryDocumentLinks("ARTICLE", article.id);
      setDocumentLinks(updated);
      setDocFormOpen(false);
      setDocRef("");
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setDocSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="flex h-64 items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-text-muted" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex h-64 items-center justify-center p-6">
        <p className="text-sm text-status-danger">{error}</p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col space-y-4 overflow-auto p-4">
      <EntityDetailHeader
        code={article.article_code}
        designation={article.article_name}
        statusSlot={
          <Badge variant={article.is_active === 1 ? "secondary" : "outline"}>
            {article.is_active === 1 ? tc("status.active") : tc("status.inactive")}
          </Badge>
        }
        qrSlot={
          <EntityQrCode
            code={article.article_code}
            name={article.article_name}
            payload={`maintafox://article/${article.id}`}
            viewPermission="inv.view"
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
            <PermissionGate permission="inv.manage">
              <Button
                variant="default"
                size="sm"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={() => setAdjustOpen(true)}
              >
                <Package className="h-3.5 w-3.5" />
                {t("detail.actions.adjustStock")}
              </Button>
            </PermissionGate>
            <PermissionGate permission="inv.procure">
              <Button
                variant="outline"
                size="sm"
                className={ENTITY_DETAIL_HEADER_ACTION_BTN}
                onClick={onCreateRequisition}
              >
                <ClipboardList className="h-3.5 w-3.5" />
                {t("detail.actions.createRequisition")}
              </Button>
            </PermissionGate>
          </>
        }
        overflowSlot={
          <PermissionGate permission="inv.manage">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 w-7 px-0"
                  aria-label={t("detail.actions.more")}
                >
                  <MoreHorizontal className="h-3.5 w-3.5" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-48">
                <DropdownMenuItem className="gap-2" onClick={onEdit}>
                  <Pencil className="h-3.5 w-3.5" />
                  {tc("action.edit")}
                </DropdownMenuItem>
                {article.is_active === 1 ? (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      className="gap-2 text-status-danger focus:text-status-danger"
                      onClick={onDeactivate}
                    >
                      <AlertTriangle className="h-3.5 w-3.5" />
                      {t("detail.actions.deactivate")}
                    </DropdownMenuItem>
                  </>
                ) : null}
              </DropdownMenuContent>
            </DropdownMenu>
          </PermissionGate>
        }
        footerSlot={
          <EntitySummaryKpiStrip
            title={t("detail.kpis.title")}
            kpis={[
              { id: "onHand", label: t("detail.kpis.onHand"), value: formatQty(kpis.onHand) },
              { id: "reserved", label: t("detail.kpis.reserved"), value: formatQty(kpis.reserved) },
              { id: "available", label: t("detail.kpis.available"), value: formatQty(kpis.available) },
              { id: "locations", label: t("detail.kpis.locations"), value: String(kpis.locations) },
              {
                id: "movements30d",
                label: t("detail.kpis.movements30d"),
                value: String(kpis.movements30d),
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
            <EntityActivityTimeline
              items={activityItems}
              emptyLabel={t("detail.activity.empty")}
              todayLabel={t("detail.activity.today")}
              searchPlaceholder={t("detail.activity.search")}
              kindAllLabel={t("detail.activity.kindAll")}
              periodAllLabel={t("detail.activity.periodAll")}
              period30dLabel={t("detail.activity.period30d")}
              period90dLabel={t("detail.activity.period90d")}
              typeLabel={(key) => t(`detail.activity.types.${key}`, { defaultValue: key })}
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
          </CardContent>
        </Card>
      ) : null}

      {/* ── STOCK POSITIONS ── */}
      {activeView === "stock_positions" ? (
        <Card>
          <CardContent className="space-y-2 pt-4">
            {balances.length === 0 ? (
              <p className="text-sm text-text-muted">{t("detail.stockPositions.empty")}</p>
            ) : (
              balances.map((b) => (
                <div key={b.id} className="rounded border border-surface-border p-2.5 text-sm">
                  <div className="font-medium">
                    {formatSiteLabel(b.warehouse_code, b.warehouse_name)}
                    <span className="text-text-muted"> / </span>
                    {formatSiteLabel(b.location_code, b.location_name)}
                  </div>
                  <div className="mt-1 text-xs text-text-muted">
                    {t("detail.kpis.onHand")}: {formatQty(b.on_hand_qty)} ·{" "}
                    {t("detail.kpis.reserved")}: {formatQty(b.reserved_qty)} ·{" "}
                    {t("detail.kpis.available")}: {formatQty(b.available_qty)}
                  </div>
                </div>
              ))
            )}
          </CardContent>
        </Card>
      ) : null}

      {/* ── RESERVATIONS ── */}
      {activeView === "reservations" ? (
        <Card>
          <CardContent className="space-y-4 pt-4">
            <ReservationInsightsPanel reservations={reservations} />
            <ArticleConsumptionChart articleId={article.id} />
          </CardContent>
        </Card>
      ) : null}

      {/* ── PURCHASE HISTORY ── */}
      {activeView === "purchase_history" ? (
        <Card>
          <CardContent className="pt-4">
            {purchaseHistory.length === 0 ? (
              <p className="text-sm text-text-muted">{t("article.purchaseHistory.empty")}</p>
            ) : (
              <div className="space-y-2">
                {purchaseHistory.map((row) => (
                  <div key={row.purchase_order_id} className="rounded border border-surface-border p-2.5 text-sm">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono font-medium">{row.po_number}</span>
                      <Badge variant="outline" className="h-4 text-[10px]">
                        {row.status}
                      </Badge>
                      {row.supplier_name ? (
                        <span className="text-xs text-text-muted">{row.supplier_name}</span>
                      ) : null}
                    </div>
                    <div className="mt-1 flex flex-wrap gap-4 text-xs text-text-muted">
                      <span>
                        {t("article.purchaseHistory.columns.orderedQty")}: {row.ordered_qty}
                      </span>
                      <span>
                        {t("article.purchaseHistory.columns.receivedQty")}: {row.received_qty}
                      </span>
                      {row.unit_price != null ? (
                        <span>
                          {t("article.purchaseHistory.columns.unitPrice")}: {row.unit_price.toFixed(2)}
                        </span>
                      ) : null}
                      {row.ordered_at ? (
                        <span>{formatDate(row.ordered_at)}</span>
                      ) : null}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      ) : null}

      {/* ── EQUIVALENTS ── */}
      {activeView === "equivalents" ? (
        <Card>
          <CardContent className="pt-4">
            <div className="mb-3 flex items-center justify-between">
              <p className="text-sm font-medium">{t("article.sections.equivalents")}</p>
              <PermissionGate permission="inv.manage">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setEquivArticleId(0);
                    setEquivType("SUBSTITUTE");
                    setEquivNotes("");
                    setEquivBidirectional(true);
                    setEquivFormOpen(true);
                  }}
                >
                  {t("article.actions.addEquivalent")}
                </Button>
              </PermissionGate>
            </div>
            {equivalents.length === 0 ? (
              <p className="text-sm text-text-muted">{t("article.equivalents.empty")}</p>
            ) : (
              <div className="space-y-2">
                {equivalents.map((eq) => (
                  <div
                    key={eq.id}
                    className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-2.5 text-sm"
                  >
                    <span className="font-medium">
                      {eq.equivalent_code} — {eq.equivalent_name}
                    </span>
                    <Badge variant="outline" className="h-4 text-[10px]">
                      {eq.equivalence_type}
                    </Badge>
                    {eq.is_bidirectional === 1 ? (
                      <span className="text-xs text-text-muted">↔</span>
                    ) : null}
                    {eq.notes ? (
                      <span className="text-xs text-text-muted">{eq.notes}</span>
                    ) : null}
                    <PermissionGate permission="inv.manage">
                      <Button
                        size="sm"
                        variant="ghost"
                        className="ml-auto h-5 px-1.5 text-xs text-status-danger hover:text-status-danger"
                        onClick={() => void deleteEquivalent(eq.id)}
                      >
                        Remove
                      </Button>
                    </PermissionGate>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      ) : null}

      {/* ── REPAIRABLE HISTORY ── */}
      {activeView === "repairable_history" ? (
        <Card>
          <CardContent className="pt-4">
            {repairableHistory.length === 0 ? (
              <p className="text-sm text-text-muted">{t("article.repairableHistory.empty")}</p>
            ) : (
              <div className="space-y-2">
                {repairableHistory.map((row) => (
                  <div
                    key={row.order_id}
                    className="rounded border border-surface-border p-2.5 text-sm"
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="font-mono font-medium">{row.order_code}</span>
                      <Badge variant="secondary" className="h-4 text-[10px]">
                        {row.status}
                      </Badge>
                      <span className="tabular-nums">×{row.quantity}</span>
                    </div>
                    <div className="mt-1 flex flex-wrap gap-4 text-xs text-text-muted">
                      {row.reason ? <span>{row.reason}</span> : null}
                      {row.repair_cost != null ? (
                        <span>
                          {t("article.repairableHistory.columns.cost")}: {row.repair_cost.toFixed(2)}
                        </span>
                      ) : null}
                      <span>{formatDate(row.created_at)}</span>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      ) : null}

      {/* ── DOCUMENTS ── */}
      {activeView === "documents" ? (
        <Card>
          <CardContent className="pt-4">
            <div className="mb-3 flex items-center justify-between">
              <p className="text-sm font-medium">{t("article.sections.documents")}</p>
              <PermissionGate permission="inv.manage">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setDocRef("");
                    setDocPurpose("DATASHEET");
                    setDocPrimary(false);
                    setDocFormOpen(true);
                  }}
                >
                  {t("article.actions.addDocumentLink")}
                </Button>
              </PermissionGate>
            </div>
            {documentLinks.length === 0 ? (
              <p className="text-sm text-text-muted">{t("article.documents.empty")}</p>
            ) : (
              <div className="space-y-2">
                {documentLinks.map((doc) => (
                  <div
                    key={doc.id}
                    className="flex flex-wrap items-center gap-2 rounded border border-surface-border p-2.5 text-sm"
                  >
                    <span className="break-all font-medium">{doc.document_ref}</span>
                    <Badge variant="outline" className="h-4 text-[10px]">
                      {doc.link_purpose}
                    </Badge>
                    {doc.is_primary === 1 ? (
                      <Badge variant="secondary" className="h-4 text-[10px]">
                        Primary
                      </Badge>
                    ) : null}
                    <span className="text-xs text-text-muted">{formatDate(doc.valid_from)}</span>
                  </div>
                ))}
              </div>
            )}
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
              label={tc("label.status")}
              value={article.is_active === 1 ? tc("status.active") : tc("status.inactive")}
            />
          </DetailSectionCard>

          <DetailSectionCard title={t("detail.sections.inventory")} icon={Package}>
            <DetailFieldRow label={t("detail.fields.minStock")} value={formatQty(article.min_stock)} />
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
              value={article.created_at ? new Date(article.created_at).toLocaleString() : "—"}
            />
            <DetailFieldRow
              label={tc("label.updatedAt")}
              value={article.updated_at ? new Date(article.updated_at).toLocaleString() : "—"}
            />
          </DetailSectionCard>

          <SupplierArticleSourcesSection
            articleId={article.id}
            suppliers={sourcingSuppliers}
            onChanged={handleSourcesChanged}
            refreshToken={sourcingVersion}
          />

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
              <Label>{t("article.equivalents.fields.equivalentArticle")}</Label>
              <Select
                value={String(equivArticleId)}
                onValueChange={(v) => setEquivArticleId(Number(v))}
              >
                <SelectTrigger>
                  <SelectValue placeholder="Select article" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">Select article</SelectItem>
                  {allArticles.map((a) => (
                    <SelectItem key={a.id} value={String(a.id)}>
                      {a.article_code} — {a.article_name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label>{t("article.equivalents.fields.type")}</Label>
              <Select value={equivType} onValueChange={setEquivType}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {EQUIVALENCE_TYPES.map((et) => (
                    <SelectItem key={et} value={et}>
                      {et}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center gap-2">
              <input
                id="equiv-bidirectional"
                type="checkbox"
                checked={equivBidirectional}
                onChange={(e) => setEquivBidirectional(e.target.checked)}
              />
              <Label htmlFor="equiv-bidirectional">
                {t("article.equivalents.fields.bidirectional")}
              </Label>
            </div>
            <div className="space-y-1">
              <Label>{t("article.equivalents.fields.notes")}</Label>
              <Input
                value={equivNotes}
                onChange={(e) => setEquivNotes(e.target.value)}
              />
            </div>
          </div>
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="outline" onClick={() => setEquivFormOpen(false)}>
              Cancel
            </Button>
            <Button
              disabled={equivSaving || equivArticleId <= 0}
              onClick={() => void saveEquivalent()}
            >
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── DOCUMENT LINK FORM DIALOG ── */}
      <Dialog open={docFormOpen} onOpenChange={setDocFormOpen}>
        <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
          <DialogHeader>
            <DialogTitle>{t("article.actions.addDocumentLink")}</DialogTitle>
          </DialogHeader>
          <div className="grid gap-3">
            <div className="space-y-1">
              <Label>{t("procurement.suppliers.documents.fields.ref")}</Label>
              <Input
                value={docRef}
                onChange={(e) => setDocRef(e.target.value)}
                placeholder="e.g. spec-2024.pdf or https://…"
              />
            </div>
            <div className="space-y-1">
              <Label>{t("procurement.suppliers.documents.fields.purpose")}</Label>
              <Select value={docPurpose} onValueChange={setDocPurpose}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {DOC_LINK_PURPOSES.map((p) => (
                    <SelectItem key={p} value={p}>
                      {p}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="flex items-center gap-2">
              <input
                id="doc-primary"
                type="checkbox"
                checked={docPrimary}
                onChange={(e) => setDocPrimary(e.target.checked)}
              />
              <Label htmlFor="doc-primary">
                {t("procurement.suppliers.documents.fields.primary")}
              </Label>
            </div>
          </div>
          <DialogFooter className="gap-2 sm:gap-0">
            <Button variant="outline" onClick={() => setDocFormOpen(false)}>
              Cancel
            </Button>
            <Button
              disabled={docSaving || !docRef.trim()}
              onClick={() => void saveDocLink()}
            >
              Save
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
