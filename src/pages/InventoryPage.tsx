import type { ColumnDef } from "@tanstack/react-table";
import { AlertTriangle, Columns3, List, PackageSearch, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { ArticleDetailWorkspace } from "@/components/inventory/ArticleDetailWorkspace";
import {
  ArticleEditorFields,
  type ExtendedArticleInput,
} from "@/components/inventory/ArticleEditorFields";
import { InventoryControlsPanel } from "@/components/inventory/InventoryControlsPanel";
import { InventoryIssueDesk } from "@/components/inventory/InventoryIssueDesk";
import { InventoryValuationPanel } from "@/components/inventory/InventoryValuationPanel";
import {
  type ProcurementPanelRequest,
  ProcurementRepairablePanel,
  type ProcurementRepairablePanelHandle,
} from "@/components/inventory/ProcurementRepairablePanel";
import { ProcurementRulesPanel } from "@/components/inventory/ProcurementRulesPanel";
import { UnitConversionsPanel } from "@/components/inventory/UnitConversionsPanel";
import {
  WarehouseLocationPanel,
  type WarehouseLocationPanelHandle,
} from "@/components/inventory/WarehouseLocationPanel";
import {
  RefreshButton,
  type ViewOption,
  ViewToggle,
} from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { useMeasureUnits } from "@/components/lookups/MeasureUnitPicker";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { LIST_PARAM, withoutParams } from "@/lib/list-links";
import { cn } from "@/lib/utils";
import {
  type InventoryArticleRow,
  articleToInput,
  calculateInventoryAbc,
  calculateInventoryXyz,
  getInventoryValuationReport,
  isBelowReorderPoint,
} from "@/services/inventory-service";
import { getLookupValues } from "@/services/lookup-service";
import { pushAppToast } from "@/store/app-toast-store";
import { useInventoryStore } from "@/stores/inventory-store";
import type { LookupValueOption } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const EMPTY_ARTICLE_FORM: ExtendedArticleInput = {
  article_code: "",
  article_name: "",
  family_id: null,
  unit_value_id: 0,
  criticality_value_id: null,
  stocking_type_value_id: 0,
  tax_category_value_id: 0,
  procurement_category_value_id: null,
  preferred_warehouse_id: null,
  preferred_location_id: null,
  min_stock: 0,
  max_stock: null,
  reorder_point: 0,
  safety_stock: 0,
  is_active: true,
  // Extended item-master fields
  manufacturer_name: null,
  manufacturer_part_number: null,
  oem_part_number: null,
  replenishment_policy_code: null,
  economic_order_qty: null,
  minimum_order_qty: null,
  maximum_order_qty: null,
  order_multiple_qty: null,
  lead_time_days: null,
  review_period_days: null,
  abc_class_code: null,
  xyz_class_code: null,
  is_critical_spare: null,
  requires_expiration: null,
  shelf_life_days: null,
  requires_batch_tracking: null,
  barcode: null,
  requires_serial_tracking: null,
};

type InventoryTab =
  | "master"
  | "issueDesk"
  | "topology"
  | "procurement"
  | "controls"
  | "valuation"
  | "rules"
  | "units";
type ProcurementView = "list" | "kanban";

function readProcurementView(): ProcurementView {
  try {
    return localStorage.getItem("inv-procurement-view") === "kanban" ? "kanban" : "list";
  } catch {
    return "list";
  }
}

export function InventoryPage() {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatCurrency } = useFormatters();
  const families = useInventoryStore((s) => s.families);
  const warehouses = useInventoryStore((s) => s.warehouses);
  const locations = useInventoryStore((s) => s.locations);
  const articles = useInventoryStore((s) => s.articles);
  const loading = useInventoryStore((s) => s.loading);
  const saving = useInventoryStore((s) => s.saving);
  const loadError = useInventoryStore((s) => s.loadError);
  const loadAll = useInventoryStore((s) => s.loadAll);
  const setArticleSearch = useInventoryStore((s) => s.setArticleSearch);
  const createArticle = useInventoryStore((s) => s.createArticle);
  const updateArticle = useInventoryStore((s) => s.updateArticle);

  const [criticalityOptions, setCriticalityOptions] = useState<LookupValueOption[]>([]);
  const [stockingTypeOptions, setStockingTypeOptions] = useState<LookupValueOption[]>([]);
  const [taxCategoryOptions, setTaxCategoryOptions] = useState<LookupValueOption[]>([]);
  const [procurementCategoryOptions, setProcurementCategoryOptions] = useState<LookupValueOption[]>(
    [],
  );
  const [lookupError, setLookupError] = useState<unknown>(null);
  const [lookupVersion, setLookupVersion] = useState(0);
  // Stock units: the governed reference list MEASURE.UNITS (Données de référence).
  const { units: unitOptions, error: unitsError } = useMeasureUnits(true, lookupVersion);
  const [articleForm, setArticleForm] = useState<ExtendedArticleInput>(EMPTY_ARTICLE_FORM);
  const [selectedArticle, setSelectedArticle] = useState<InventoryArticleRow | null>(null);
  const [isEditingInDialog, setEditingInDialog] = useState(false);
  const [deactivateOpen, setDeactivateOpen] = useState(false);

  const [invTab, setInvTab] = useState<InventoryTab>("master");
  const warehousePanelRef = useRef<WarehouseLocationPanelHandle>(null);
  const procurementPanelRef = useRef<ProcurementRepairablePanelHandle>(null);
  const [procurementRequest, setProcurementRequest] = useState<ProcurementPanelRequest | null>(
    null,
  );
  const [controlsReloadToken, setControlsReloadToken] = useState(0);
  const [procurementView, setProcurementView] = useState<ProcurementView>(readProcurementView);
  const [masterSearchInput, setMasterSearchInput] = useState("");
  const [masterFamilyFilter, setMasterFamilyFilter] = useState<string | null>(null);
  const [masterStatusFilter, setMasterStatusFilter] = useState<string | null>(null);
  const [masterStockFilter, setMasterStockFilter] = useState<string | null>(null);
  const [isCreateArticleOpen, setCreateArticleOpen] = useState(false);
  const articleSearchStore = useInventoryStore((s) => s.articleSearch);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

  useEffect(() => {
    setSelectedArticle((prev) => {
      if (!prev) return null;
      return articles.find((a) => a.id === prev.id) ?? null;
    });
  }, [articles]);

  useEffect(() => {
    setMasterSearchInput(articleSearchStore);
  }, [articleSearchStore]);

  // Deep link from other modules (asset spare parts, work orders): /inventory?articleId=<id>.
  const [searchParams, setSearchParams] = useSearchParams();
  const linkedArticleId = Number(searchParams.get("articleId"));

  // A link may ask for the articles below their reorder point (?stock=below_reorder): it opens
  // the article list with that stock filter as a visible chip, then leaves the address.
  useEffect(() => {
    const stock = searchParams.get(LIST_PARAM.stock);
    if (stock !== "below_reorder") return;
    setInvTab("master");
    setMasterStockFilter("below_reorder");
    setSearchParams(withoutParams(searchParams, LIST_PARAM.stock), { replace: true });
  }, [searchParams, setSearchParams]);
  // The list starts empty: only resolve the link once a load has run and finished.
  const articleLoadSeenRef = useRef(false);
  useEffect(() => {
    if (loading) articleLoadSeenRef.current = true;
  }, [loading]);
  useEffect(() => {
    if (!Number.isInteger(linkedArticleId) || linkedArticleId <= 0) return;
    if (loading || !articleLoadSeenRef.current) return;
    const linked = articles.find((a) => a.id === linkedArticleId);
    if (!linked && articleSearchStore) {
      // The current search may hide the article: clear it, the list reloads and this runs again.
      void setArticleSearch("");
      return;
    }
    if (linked) {
      setInvTab("master");
      setSelectedArticle(linked);
    }
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.delete("articleId");
        return next;
      },
      { replace: true },
    );
  }, [linkedArticleId, loading, articles, articleSearchStore, setArticleSearch, setSearchParams]);

  useEffect(() => {
    let cancelled = false;
    setLookupError(null);
    Promise.all([
      getLookupValues("equipment.criticality"),
      getLookupValues("inventory.stocking_type"),
      getLookupValues("inventory.tax_category"),
      getLookupValues("inventory.procurement_category"),
    ])
      .then(([crit, stockingTypes, taxCategories, procurementCategories]) => {
        if (cancelled) return;
        const activeStockingTypes = stockingTypes.filter((item) => item.is_active !== 0);
        const activeTaxCategories = taxCategories.filter((item) => item.is_active !== 0);
        setCriticalityOptions(crit.filter((c) => c.is_active !== 0));
        setStockingTypeOptions(activeStockingTypes);
        setTaxCategoryOptions(activeTaxCategories);
        setProcurementCategoryOptions(procurementCategories.filter((item) => item.is_active !== 0));
        // No value is pre-chosen: the unit, stocking type and tax category of a new article are
        // picked by the person (Save stays disabled with the reason until they are).
      })
      .catch((err: unknown) => {
        if (!cancelled) setLookupError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [lookupVersion]);

  useEffect(() => {
    if (unitsError) setLookupError(unitsError);
  }, [unitsError]);

  const formatQty = useCallback(
    (value: number) => formatNumber(value, { maximumFractionDigits: 3 }),
    [formatNumber],
  );

  const articleColumns: ColumnDef<InventoryArticleRow>[] = useMemo(
    () => [
      {
        accessorKey: "article_code",
        header: t("list.columns.code"),
        cell: ({ row }) => <span className="font-mono text-xs">{row.original.article_code}</span>,
      },
      {
        accessorKey: "article_name",
        header: t("list.columns.name"),
        cell: ({ row }) => (
          <span className="block max-w-[220px] truncate" title={row.original.article_name}>
            {row.original.article_name}
          </span>
        ),
      },
      {
        id: "family",
        accessorFn: (a) => a.family_name ?? a.family_code ?? "",
        header: t("list.columns.family"),
        cell: ({ row }) => (
          <span className="text-xs text-text-muted">
            {row.original.family_name ?? row.original.family_code ?? "—"}
          </span>
        ),
      },
      {
        accessorKey: "on_hand_qty",
        header: t("list.columns.onHand"),
        cell: ({ row }) => (
          <span className="tabular-nums">
            {formatQty(row.original.on_hand_qty)}{" "}
            <span className="text-xs text-text-muted">{row.original.unit_code}</span>
          </span>
        ),
      },
      {
        accessorKey: "reserved_qty",
        header: t("list.columns.reserved"),
        cell: ({ row }) => (
          <span className="tabular-nums text-text-secondary">
            {formatQty(row.original.reserved_qty)}
          </span>
        ),
      },
      {
        accessorKey: "available_qty",
        header: t("list.columns.available"),
        cell: ({ row }) => {
          const low = isBelowReorderPoint(row.original);
          return (
            <span
              className={cn(
                "inline-flex items-center gap-1 tabular-nums",
                low && cn("font-semibold", mfTone.warning.text),
              )}
              title={
                low
                  ? t("list.belowReorderHint", {
                      reorderPoint: formatQty(row.original.reorder_point),
                    })
                  : undefined
              }
            >
              {low ? <AlertTriangle className="h-3.5 w-3.5" aria-hidden /> : null}
              {formatQty(row.original.available_qty)}
              {low ? <span className="sr-only">{t("list.belowReorder")}</span> : null}
            </span>
          );
        },
      },
      {
        accessorKey: "avg_unit_cost",
        header: t("list.columns.avgCost"),
        cell: ({ row }) => (
          <span className="tabular-nums text-text-secondary">
            {row.original.avg_unit_cost != null ? formatCurrency(row.original.avg_unit_cost) : "—"}
          </span>
        ),
      },
      {
        accessorKey: "stock_value",
        header: t("list.columns.stockValue"),
        cell: ({ row }) => {
          const { stock_value: value, unvalued_qty: unvalued, unit_code: unit } = row.original;
          return (
            <span
              className="tabular-nums"
              title={
                unvalued > 0
                  ? t("list.unvaluedHint", { qty: formatQty(unvalued), unit })
                  : undefined
              }
            >
              {value != null ? formatCurrency(value) : "—"}
              {unvalued > 0 ? <span className="text-text-muted"> *</span> : null}
              {unvalued > 0 ? (
                <span className="sr-only">
                  {t("list.unvaluedHint", { qty: formatQty(unvalued), unit })}
                </span>
              ) : null}
            </span>
          );
        },
      },
      {
        accessorKey: "reorder_point",
        header: t("list.columns.reorderPoint"),
        cell: ({ row }) => (
          <span className="tabular-nums text-text-secondary">
            {row.original.reorder_point > 0 ? formatQty(row.original.reorder_point) : "—"}
          </span>
        ),
      },
      {
        accessorKey: "is_active",
        header: t("list.columns.status"),
        cell: ({ row }) =>
          row.original.is_active === 1 ? (
            isBelowReorderPoint(row.original) ? (
              <StatusChip label={t("list.belowReorder")} tone="warning" dot />
            ) : (
              <StatusChip label={t("filters.statusActive")} tone="success" dot />
            )
          ) : (
            <StatusChip label={t("filters.statusInactive")} tone="neutral" dot />
          ),
      },
    ],
    [formatCurrency, formatQty, t],
  );

  /**
   * Stock value of the whole store, from the valuation report (backend total, same figure as the
   * Valuation tab) — not a sum over the articles the search happens to show. When it cannot be
   * read the header says so instead of showing a partial figure.
   */
  const [stockValue, setStockValue] = useState<{
    total: number;
    currency: string;
    unvalued: boolean;
  } | null>(null);
  const [stockValueFailed, setStockValueFailed] = useState(false);
  const [stockValueVersion, setStockValueVersion] = useState(0);
  useEffect(() => {
    let cancelled = false;
    setStockValueFailed(false);
    getInventoryValuationReport("family")
      .then((r) => {
        if (!cancelled) {
          setStockValue({
            total: r.total_value,
            currency: r.currency_code,
            unvalued: r.unvalued_qty > 0,
          });
        }
      })
      .catch(() => {
        if (!cancelled) {
          setStockValue(null);
          setStockValueFailed(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [stockValueVersion]);

  const stockValueSummary = useMemo(() => {
    if (stockValueFailed) return t("page.stockValueUnavailable");
    if (!stockValue) return null;
    const value = t("page.stockValueTotal", {
      value: formatCurrency(stockValue.total, stockValue.currency),
    });
    return stockValue.unvalued ? `${value} · ${t("page.stockValueSomeUnvalued")}` : value;
  }, [stockValue, stockValueFailed, formatCurrency, t]);

  const resetArticleForm = () => {
    setArticleForm(EMPTY_ARTICLE_FORM);
  };

  const openArticleDetails = (article: InventoryArticleRow) => {
    setSelectedArticle((prev) => (prev?.id === article.id ? null : article));
    setEditingInDialog(false);
  };

  const beginEditSelectedArticle = () => {
    if (!selectedArticle) return;
    setArticleForm(articleToInput(selectedArticle));
    setEditingInDialog(true);
  };

  const saveEditedArticle = async () => {
    if (!selectedArticle) return;
    try {
      await updateArticle(selectedArticle.id, selectedArticle.row_version, articleForm);
    } catch {
      // The global IPC toast reports the failure; keep the dialog open with the user's input.
      return;
    }
    const refreshed = useInventoryStore
      .getState()
      .articles.find((article) => article.id === selectedArticle.id);
    setSelectedArticle(refreshed ?? null);
    setEditingInDialog(false);
  };

  const deactivateSelectedArticle = async () => {
    if (!selectedArticle) return;
    // Rejects on failure: ConfirmDialog stays open and the global toast explains why.
    await updateArticle(
      selectedArticle.id,
      selectedArticle.row_version,
      articleToInput(selectedArticle, { is_active: false }),
    );
    setSelectedArticle(null);
  };

  /** Recalculates ABC or XYZ classes of every article, then reloads the edited one. */
  const recalculateClasses = async (kind: "abc" | "xyz") => {
    let updated: number;
    try {
      updated = kind === "abc" ? await calculateInventoryAbc() : await calculateInventoryXyz();
    } catch {
      return; // reported by the global IPC toast
    }
    await loadAll();
    const editedId = selectedArticle?.id;
    const refreshed =
      editedId != null
        ? useInventoryStore.getState().articles.find((article) => article.id === editedId)
        : undefined;
    if (refreshed) {
      setSelectedArticle(refreshed);
      setArticleForm((form) => ({
        ...form,
        abc_class_code: refreshed.abc_class_code,
        xyz_class_code: refreshed.xyz_class_code,
      }));
    }
    pushAppToast({
      title:
        updated > 0
          ? t(
              kind === "abc" ? "article.classification.abcDone" : "article.classification.xyzDone",
              {
                count: updated,
              },
            )
          : t("article.classification.noMovement"),
      variant: updated > 0 ? "success" : "default",
    });
  };

  const handleCreateRequisitionFromDetail = () => {
    if (!selectedArticle) return;
    setProcurementRequest({
      kind: "create-requisition",
      articleId: selectedArticle.id,
      locationId: selectedArticle.preferred_location_id,
    });
    setInvTab("procurement");
  };

  /** Procurement surfaces → open the article master record. */
  const handleOpenArticleFromProcurement = (articleId: number) => {
    const article = articles.find((row) => row.id === articleId);
    if (!article) return;
    setSelectedArticle(article);
    setEditingInDialog(false);
    setInvTab("master");
  };

  /** Article purchase history → open the PO inside the procurement tab. */
  const handleOpenPurchaseOrderFromDetail = (purchaseOrderId: number) => {
    setProcurementRequest({ kind: "open-purchase-order", purchaseOrderId });
    setInvTab("procurement");
  };

  const createNewArticle = async () => {
    try {
      await createArticle(articleForm);
    } catch {
      return; // global IPC toast; keep the dialog open with the user's input
    }
    resetArticleForm();
    setCreateArticleOpen(false);
  };

  const onMasterSearchChange = useCallback(
    (val: string) => {
      void setArticleSearch(val.trim());
    },
    [setArticleSearch],
  );

  const resetMasterFilters = useCallback(() => {
    setMasterSearchInput("");
    setMasterFamilyFilter(null);
    setMasterStatusFilter(null);
    setMasterStockFilter(null);
    void setArticleSearch("");
  }, [setArticleSearch]);

  const masterFilterDefs = useMemo<SmartFilterDef[]>(
    () => [
      {
        id: "family",
        kind: "select",
        label: t("filters.family"),
        options: families
          .filter((f) => f.is_active === 1)
          .map((f) => ({ value: String(f.id), label: `${f.code} — ${f.name}` })),
        value: masterFamilyFilter,
        onChange: setMasterFamilyFilter,
      },
      {
        id: "stock",
        kind: "select",
        label: t("filters.stock"),
        options: [
          { value: "below_reorder", label: t("filters.stockBelowReorder") },
          { value: "out_of_stock", label: t("filters.stockOut") },
        ],
        value: masterStockFilter,
        onChange: setMasterStockFilter,
      },
      {
        id: "status",
        kind: "select",
        label: t("filters.status"),
        options: [
          { value: "active", label: t("filters.statusActive") },
          { value: "inactive", label: t("filters.statusInactive") },
        ],
        value: masterStatusFilter,
        onChange: setMasterStatusFilter,
      },
    ],
    [families, masterFamilyFilter, masterStatusFilter, masterStockFilter, t],
  );

  const filteredArticles = useMemo(() => {
    return articles.filter((article) => {
      if (masterFamilyFilter && String(article.family_id ?? "") !== masterFamilyFilter) {
        return false;
      }
      if (masterStatusFilter === "active" && article.is_active !== 1) return false;
      if (masterStatusFilter === "inactive" && article.is_active === 1) return false;
      if (masterStockFilter === "below_reorder" && !isBelowReorderPoint(article)) return false;
      if (masterStockFilter === "out_of_stock" && article.available_qty > 0) return false;
      return true;
    });
  }, [articles, masterFamilyFilter, masterStatusFilter, masterStockFilter]);

  const switchProcurementView = useCallback((v: ProcurementView) => {
    setProcurementView(v);
    try {
      localStorage.setItem("inv-procurement-view", v);
    } catch {
      // Remembering the view is a convenience only.
    }
  }, []);

  const procurementViewOptions = useMemo<ViewOption<ProcurementView>[]>(
    () => [
      { value: "list", label: t("page.viewList"), icon: List },
      { value: "kanban", label: t("page.viewKanban"), icon: Columns3 },
    ],
    [t],
  );

  const isArticleFormValid = useMemo(() => {
    const maxStock = articleForm.max_stock ?? null;
    if (!articleForm.article_code.trim() || !articleForm.article_name.trim()) return false;
    if (articleForm.unit_value_id <= 0) return false;
    if (articleForm.stocking_type_value_id <= 0) return false;
    if (articleForm.tax_category_value_id <= 0) return false;
    if (articleForm.min_stock < 0) return false;
    if (articleForm.reorder_point < 0) return false;
    if (articleForm.safety_stock < 0) return false;
    if (articleForm.reorder_point < articleForm.min_stock) return false;
    if (maxStock !== null && maxStock < articleForm.min_stock) return false;
    if (maxStock !== null && maxStock < articleForm.reorder_point) return false;
    if (articleForm.preferred_warehouse_id === null && articleForm.preferred_location_id !== null)
      return false;
    if (
      articleForm.preferred_warehouse_id !== null &&
      articleForm.preferred_location_id !== null &&
      locations.find((l) => l.id === articleForm.preferred_location_id)?.warehouse_id !==
        articleForm.preferred_warehouse_id
    ) {
      return false;
    }
    return true;
  }, [articleForm, locations]);

  /** Why "Save" is disabled, shown under the form (null when the form is valid). */
  const articleFormBlocker = useMemo(() => {
    if (isArticleFormValid) return null;
    if (!articleForm.article_code.trim() || !articleForm.article_name.trim()) {
      return t("article.validation.codeAndName");
    }
    if (
      articleForm.unit_value_id <= 0 ||
      articleForm.stocking_type_value_id <= 0 ||
      articleForm.tax_category_value_id <= 0
    ) {
      return t("article.validation.requiredLists");
    }
    if (articleForm.reorder_point < articleForm.min_stock) {
      return t("article.validation.reorderBelowMin");
    }
    if (
      articleForm.max_stock != null &&
      (articleForm.max_stock < articleForm.min_stock ||
        articleForm.max_stock < articleForm.reorder_point)
    ) {
      return t("article.validation.maxBelowThresholds");
    }
    if (articleForm.preferred_location_id !== null) {
      return t("article.validation.locationOutsideWarehouse");
    }
    return t("article.validation.negative");
  }, [articleForm, isArticleFormValid, t]);

  const preferredWarehouseLocations = useMemo(
    () =>
      articleForm.preferred_warehouse_id === null
        ? []
        : locations.filter(
            (location) =>
              location.warehouse_id === articleForm.preferred_warehouse_id &&
              location.is_active === 1,
          ),
    [articleForm.preferred_warehouse_id, locations],
  );

  const refresh = () => {
    setStockValueVersion((v) => v + 1);
    if (invTab === "procurement") {
      void procurementPanelRef.current?.reload();
    }
    if (
      invTab === "controls" ||
      invTab === "issueDesk" ||
      invTab === "valuation" ||
      invTab === "rules"
    ) {
      setControlsReloadToken((v) => v + 1);
    }
    void loadAll();
  };

  const editorProps = {
    articleForm,
    setArticleForm,
    families,
    warehouses,
    preferredWarehouseLocations,
    unitOptions,
    criticalityOptions,
    stockingTypeOptions,
    taxCategoryOptions,
    procurementCategoryOptions,
  };

  const headerActions = (
    <>
      {invTab === "master" ? (
        <PermissionGate permission={P.INV_MANAGE}>
          <Button
            className={mfLayout.primaryAction}
            onClick={() => {
              resetArticleForm();
              setCreateArticleOpen(true);
            }}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("page.actions.createArticle")}
          </Button>
        </PermissionGate>
      ) : null}
      {invTab === "topology" ? (
        <PermissionGate permission={P.INV_MANAGE}>
          <Button
            className={mfLayout.primaryAction}
            onClick={() => warehousePanelRef.current?.openCreateWarehouse()}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("topology.actions.createWarehouse")}
          </Button>
        </PermissionGate>
      ) : null}
      {invTab === "procurement" ? (
        <PermissionGate permission={P.INV_PROCURE}>
          <Button
            className={mfLayout.primaryAction}
            onClick={() => procurementPanelRef.current?.openCreateRequisition()}
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("page.actions.newRequisition")}
          </Button>
          <Button
            variant="outline"
            className={mfLayout.primaryAction}
            onClick={() => procurementPanelRef.current?.openCreatePo()}
          >
            {t("procurement.purchaseOrders.create")}
          </Button>
          <Button
            variant="outline"
            className={mfLayout.primaryAction}
            onClick={() => procurementPanelRef.current?.openReceiveGoods()}
          >
            {t("procurement.purchaseOrders.actions.receive")}
          </Button>
          <Button
            variant="outline"
            className={mfLayout.primaryAction}
            onClick={() => procurementPanelRef.current?.openRepairable()}
          >
            {t("procurement.repairables.create")}
          </Button>
        </PermissionGate>
      ) : null}
      {invTab === "procurement" ? (
        <ViewToggle
          value={procurementView}
          options={procurementViewOptions}
          onChange={switchProcurementView}
        />
      ) : null}
      <RefreshButton onRefresh={refresh} loading={loading} />
    </>
  );

  return (
    <ModulePageShell
      icon={PackageSearch}
      title={t("page.title")}
      // The count belongs to the article list: other tabs list other things.
      count={invTab === "master" ? articles.length : null}
      description={stockValueSummary}
      bodyClassName={null}
      actions={headerActions}
    >
      <Tabs
        value={invTab}
        onValueChange={(value) => setInvTab(value as InventoryTab)}
        className="flex min-h-0 flex-1 flex-col px-4 py-4"
      >
        {/* Daily work first, then periodic / setup screens, with a visible separation. */}
        <TabsList className="h-auto w-fit flex-wrap gap-y-1">
          <span className="px-2 text-xs font-medium text-text-muted" aria-hidden>
            {t("page.tabGroups.daily")}
          </span>
          <TabsTrigger value="master">{t("page.tabs.master")}</TabsTrigger>
          <TabsTrigger value="issueDesk">{t("page.tabs.issueDesk")}</TabsTrigger>
          <TabsTrigger value="procurement">{t("page.tabs.procurement")}</TabsTrigger>
          <TabsTrigger value="controls">{t("page.tabs.controls")}</TabsTrigger>
          <span className="mx-1 h-6 w-px bg-surface-border" aria-hidden />
          <span className="px-2 text-xs font-medium text-text-muted" aria-hidden>
            {t("page.tabGroups.setup")}
          </span>
          <TabsTrigger value="valuation">{t("page.tabs.valuation")}</TabsTrigger>
          <TabsTrigger value="topology">{t("page.tabs.topology")}</TabsTrigger>
          <TabsTrigger value="rules">{t("page.tabs.rules")}</TabsTrigger>
          <TabsTrigger value="units">{t("unitConversions.tab")}</TabsTrigger>
        </TabsList>

        {lookupError ? (
          <ErrorState
            className="mt-4"
            error={lookupError}
            title={t("page.lookupsLoadError")}
            onRetry={() => setLookupVersion((v) => v + 1)}
          />
        ) : null}

        <TabsContent value="master" className="mt-4 min-h-0 flex-1 data-[state=inactive]:hidden">
          {loadError && articles.length === 0 ? (
            <ErrorState
              error={loadError}
              title={t("page.loadError")}
              onRetry={() => void loadAll()}
              retrying={loading}
            />
          ) : (
            <div className={mfLayout.moduleWorkspaceSplit}>
              <div className="flex w-[58%] min-w-[420px] flex-col border-r border-surface-border">
                <SmartFilterBar
                  searchPlaceholder={t("filters.searchArticle")}
                  searchValue={masterSearchInput}
                  onSearchInputChange={setMasterSearchInput}
                  onSearchChange={onMasterSearchChange}
                  filters={masterFilterDefs}
                  resultCount={filteredArticles.length}
                  onReset={resetMasterFilters}
                />
                <div className="min-h-0 flex-1 overflow-auto p-1">
                  <DataTable
                    columns={articleColumns}
                    data={filteredArticles}
                    isLoading={loading}
                    searchable={false}
                    onRowClick={openArticleDetails}
                    isRowSelected={(row) => row.id === selectedArticle?.id}
                  />
                </div>
              </div>
              <div className="min-w-[300px] flex-1 overflow-hidden">
                {selectedArticle ? (
                  <ArticleDetailWorkspace
                    article={selectedArticle}
                    onEdit={beginEditSelectedArticle}
                    onDeactivate={() => setDeactivateOpen(true)}
                    onCreateRequisition={handleCreateRequisitionFromDetail}
                    onStockAdjusted={() => void loadAll()}
                    onOpenPurchaseOrder={handleOpenPurchaseOrderFromDetail}
                  />
                ) : (
                  <div className="flex h-full items-center justify-center p-6">
                    <p className="text-sm text-text-muted">{t("detail.noSelection")}</p>
                  </div>
                )}
              </div>
            </div>
          )}
        </TabsContent>

        <TabsContent value="units" className="mt-4 data-[state=inactive]:hidden">
          <UnitConversionsPanel unitOptions={unitOptions} articles={articles} />
        </TabsContent>

        <TabsContent value="topology" className="mt-4 min-h-0 flex-1 data-[state=inactive]:hidden">
          <WarehouseLocationPanel ref={warehousePanelRef} />
        </TabsContent>

        <TabsContent value="procurement" className="mt-4 space-y-4">
          <ProcurementRepairablePanel
            ref={procurementPanelRef}
            viewMode={procurementView}
            onViewModeChange={switchProcurementView}
            onOpenArticle={handleOpenArticleFromProcurement}
            request={procurementRequest}
            onRequestHandled={() => setProcurementRequest(null)}
          />
        </TabsContent>
        <TabsContent value="controls" className="mt-4 space-y-4">
          <InventoryControlsPanel reloadToken={controlsReloadToken} />
        </TabsContent>
        <TabsContent value="issueDesk" className="mt-4 space-y-4">
          {invTab === "issueDesk" ? <InventoryIssueDesk reloadToken={controlsReloadToken} /> : null}
        </TabsContent>
        <TabsContent value="valuation" className="mt-4 space-y-4">
          {invTab === "valuation" ? (
            <InventoryValuationPanel reloadToken={controlsReloadToken} />
          ) : null}
        </TabsContent>
        <TabsContent value="rules" className="mt-4 space-y-4">
          {invTab === "rules" ? <ProcurementRulesPanel reloadToken={controlsReloadToken} /> : null}
        </TabsContent>
      </Tabs>

      <Dialog
        open={isEditingInDialog}
        onOpenChange={(open) => {
          if (!open) setEditingInDialog(false);
        }}
      >
        <DialogContent
          className="max-h-[90vh] max-w-3xl overflow-y-auto"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle>{t("article.form.editTitle")}</DialogTitle>
            <DialogDescription>{t("article.form.editDescription")}</DialogDescription>
          </DialogHeader>
          <PermissionGate permission={P.INV_MANAGE}>
            <ArticleEditorFields {...editorProps} onRecalculateClasses={recalculateClasses} />
          </PermissionGate>
          {articleFormBlocker ? (
            <p role="status" className="text-xs text-text-muted">
              {articleFormBlocker}
            </p>
          ) : null}
          <DialogFooter className="gap-2">
            <Button variant="outline" className="h-11" onClick={() => setEditingInDialog(false)}>
              {t("common:action.cancel")}
            </Button>
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                className="h-11"
                onClick={() => void saveEditedArticle()}
                disabled={saving || !isArticleFormValid}
              >
                {t("common:action.save")}
              </Button>
            </PermissionGate>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog
        open={isCreateArticleOpen}
        onOpenChange={(open) => {
          setCreateArticleOpen(open);
          if (!open) resetArticleForm();
        }}
      >
        <DialogContent
          className="max-h-[90vh] max-w-3xl overflow-y-auto"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader>
            <DialogTitle>{t("article.form.createTitle")}</DialogTitle>
            <DialogDescription>{t("article.form.createDescription")}</DialogDescription>
          </DialogHeader>
          <PermissionGate permission={P.INV_MANAGE}>
            <ArticleEditorFields {...editorProps} />
          </PermissionGate>
          {articleFormBlocker ? (
            <p role="status" className="text-xs text-text-muted">
              {articleFormBlocker}
            </p>
          ) : null}
          <DialogFooter className="gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => setCreateArticleOpen(false)}
            >
              {t("common:action.cancel")}
            </Button>
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                type="button"
                className="h-11"
                onClick={() => void createNewArticle()}
                disabled={saving || !isArticleFormValid}
              >
                <Plus className="mr-1 h-4 w-4" aria-hidden />
                {t("page.actions.createArticle")}
              </Button>
            </PermissionGate>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deactivateOpen}
        onOpenChange={setDeactivateOpen}
        title={t("detail.deactivate.title")}
        description={t("detail.deactivate.description", {
          code: selectedArticle?.article_code ?? "",
        })}
        confirmLabel={t("detail.deactivate.confirm")}
        destructive
        onConfirm={deactivateSelectedArticle}
      />
    </ModulePageShell>
  );
}
