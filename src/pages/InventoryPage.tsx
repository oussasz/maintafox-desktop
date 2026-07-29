import type { ColumnDef } from "@tanstack/react-table";
import { Columns3, List, Package, Plus, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
import {
  ProcurementRepairablePanel,
  type ProcurementRepairablePanelHandle,
} from "@/components/inventory/ProcurementRepairablePanel";
import {
  WarehouseLocationPanel,
  type WarehouseLocationPanelHandle,
} from "@/components/inventory/WarehouseLocationPanel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";
import { getLookupValues } from "@/services/lookup-service";
import { useInventoryStore } from "@/stores/inventory-store";
import type { InventoryArticle, LookupValueOption } from "@shared/ipc-types";
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
  // Phase 2 extended fields
  manufacturer_name: null,
  manufacturer_part_number: null,
  oem_part_number: null,
  replenishment_policy_code: null,
  eoq: null,
  moq: null,
  max_order_qty: null,
  order_multiple: null,
  lead_time_days: null,
  review_period_days: null,
  abc_class_code: null,
  xyz_class_code: null,
  is_critical_spare: null,
  requires_expiration: null,
  shelf_life_days: null,
  requires_batch_tracking: null,
};

export function InventoryPage() {
  const { t } = useTranslation("inventory");
  const families = useInventoryStore((s) => s.families);
  const warehouses = useInventoryStore((s) => s.warehouses);
  const locations = useInventoryStore((s) => s.locations);
  const articles = useInventoryStore((s) => s.articles);
  const loading = useInventoryStore((s) => s.loading);
  const saving = useInventoryStore((s) => s.saving);
  const error = useInventoryStore((s) => s.error);
  const loadAll = useInventoryStore((s) => s.loadAll);
  const setArticleSearch = useInventoryStore((s) => s.setArticleSearch);
  const createArticle = useInventoryStore((s) => s.createArticle);
  const updateArticle = useInventoryStore((s) => s.updateArticle);

  const [unitOptions, setUnitOptions] = useState<LookupValueOption[]>([]);
  const [criticalityOptions, setCriticalityOptions] = useState<LookupValueOption[]>([]);
  const [stockingTypeOptions, setStockingTypeOptions] = useState<LookupValueOption[]>([]);
  const [taxCategoryOptions, setTaxCategoryOptions] = useState<LookupValueOption[]>([]);
  const [procurementCategoryOptions, setProcurementCategoryOptions] = useState<LookupValueOption[]>(
    [],
  );
  const [articleForm, setArticleForm] = useState<ExtendedArticleInput>(EMPTY_ARTICLE_FORM);
  const [selectedArticle, setSelectedArticle] = useState<InventoryArticle | null>(null);
  const [isEditingInDialog, setEditingInDialog] = useState(false);

  const [invTab, setInvTab] = useState("master");
  const warehousePanelRef = useRef<WarehouseLocationPanelHandle>(null);
  const procurementPanelRef = useRef<ProcurementRepairablePanelHandle>(null);
  const [procurementView, setProcurementView] = useState<"list" | "kanban">(
    () => (localStorage.getItem("inv-procurement-view") as "list" | "kanban") || "list",
  );
  const [masterSearchInput, setMasterSearchInput] = useState("");
  const [masterFamilyFilter, setMasterFamilyFilter] = useState<string | null>(null);
  const [masterStatusFilter, setMasterStatusFilter] = useState<string | null>(null);
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

  useEffect(() => {
    const loadLookups = async () => {
      const [units, crit, stockingTypes, taxCategories, procurementCategories] = await Promise.all([
        getLookupValues("inventory.unit_of_measure"),
        getLookupValues("equipment.criticality"),
        getLookupValues("inventory.stocking_type"),
        getLookupValues("inventory.tax_category"),
        getLookupValues("inventory.procurement_category"),
      ]);
      setUnitOptions(units.filter((u) => u.is_active !== 0));
      setCriticalityOptions(crit.filter((c) => c.is_active !== 0));
      setStockingTypeOptions(stockingTypes.filter((item) => item.is_active !== 0));
      setTaxCategoryOptions(taxCategories.filter((item) => item.is_active !== 0));
      setProcurementCategoryOptions(procurementCategories.filter((item) => item.is_active !== 0));
      const firstUnit = units[0];
      const firstStockingType = stockingTypes[0];
      const firstTaxCategory = taxCategories[0];
      setArticleForm((prev) => ({
        ...prev,
        unit_value_id: prev.unit_value_id || firstUnit?.id || 0,
        stocking_type_value_id: prev.stocking_type_value_id || firstStockingType?.id || 0,
        tax_category_value_id: prev.tax_category_value_id || firstTaxCategory?.id || 0,
      }));
    };
    void loadLookups();
  }, []);

  const articleColumns: ColumnDef<InventoryArticle>[] = useMemo(
    () => [
      { accessorKey: "article_code", header: "Code" },
      { accessorKey: "article_name", header: "Article" },
      { accessorKey: "family_code", header: "Family" },
      { accessorKey: "unit_label", header: "Unit" },
      {
        accessorKey: "criticality_code",
        header: "Criticality",
        cell: ({ row }) => row.original.criticality_code ?? "—",
      },
      { accessorKey: "reorder_point", header: "Reorder" },
      {
        accessorKey: "is_active",
        header: "Status",
        cell: ({ row }) =>
          row.original.is_active === 1 ? (
            <Badge variant="secondary">Active</Badge>
          ) : (
            <Badge>Inactive</Badge>
          ),
      },
    ],
    [],
  );

  const resetArticleForm = () => {
    setArticleForm((prev) => ({
      ...EMPTY_ARTICLE_FORM,
      unit_value_id: prev.unit_value_id || unitOptions[0]?.id || 0,
      stocking_type_value_id: prev.stocking_type_value_id || stockingTypeOptions[0]?.id || 0,
      tax_category_value_id: prev.tax_category_value_id || taxCategoryOptions[0]?.id || 0,
    }));
  };

  const openArticleDetails = (article: InventoryArticle) => {
    setSelectedArticle((prev) => (prev?.id === article.id ? null : article));
    setEditingInDialog(false);
  };

  const beginEditSelectedArticle = () => {
    if (!selectedArticle) return;
    setArticleForm({
      article_code: selectedArticle.article_code,
      article_name: selectedArticle.article_name,
      family_id: selectedArticle.family_id,
      unit_value_id: selectedArticle.unit_value_id,
      criticality_value_id: selectedArticle.criticality_value_id,
      stocking_type_value_id: selectedArticle.stocking_type_value_id,
      tax_category_value_id: selectedArticle.tax_category_value_id,
      procurement_category_value_id: selectedArticle.procurement_category_value_id,
      preferred_warehouse_id: selectedArticle.preferred_warehouse_id,
      preferred_location_id: selectedArticle.preferred_location_id,
      min_stock: selectedArticle.min_stock,
      max_stock: selectedArticle.max_stock,
      reorder_point: selectedArticle.reorder_point,
      safety_stock: selectedArticle.safety_stock,
      is_active: selectedArticle.is_active === 1,
    });
    setEditingInDialog(true);
  };

  const saveEditedArticle = async () => {
    if (!selectedArticle) return;
    await updateArticle(selectedArticle.id, selectedArticle.row_version, articleForm);
    await loadAll();
    const refreshed = useInventoryStore
      .getState()
      .articles.find((article) => article.id === selectedArticle.id);
    setSelectedArticle(refreshed ?? null);
    setEditingInDialog(false);
  };

  const softDeleteSelectedArticle = async () => {
    if (!selectedArticle) return;
    const confirmed = window.confirm(
      `Deactivate article ${selectedArticle.article_code}? It will be hidden from active operations.`,
    );
    if (!confirmed) return;
    await updateArticle(selectedArticle.id, selectedArticle.row_version, {
      article_code: selectedArticle.article_code,
      article_name: selectedArticle.article_name,
      family_id: selectedArticle.family_id,
      unit_value_id: selectedArticle.unit_value_id,
      criticality_value_id: selectedArticle.criticality_value_id,
      stocking_type_value_id: selectedArticle.stocking_type_value_id,
      tax_category_value_id: selectedArticle.tax_category_value_id,
      procurement_category_value_id: selectedArticle.procurement_category_value_id,
      preferred_warehouse_id: selectedArticle.preferred_warehouse_id,
      preferred_location_id: selectedArticle.preferred_location_id,
      min_stock: selectedArticle.min_stock,
      max_stock: selectedArticle.max_stock,
      reorder_point: selectedArticle.reorder_point,
      safety_stock: selectedArticle.safety_stock,
      is_active: false,
    });
    await loadAll();
    setSelectedArticle(null);
  };

  const handleCreateRequisitionFromDetail = () => {
    setInvTab("procurement");
    window.setTimeout(() => {
      procurementPanelRef.current?.openCreateRequisition();
    }, 0);
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
    setInvTab("procurement");
    window.setTimeout(() => {
      procurementPanelRef.current?.openPurchaseOrder(purchaseOrderId);
    }, 0);
  };

  const createNewArticle = async () => {
    await createArticle(articleForm);
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
    [families, masterFamilyFilter, masterStatusFilter, t],
  );

  const filteredArticles = useMemo(() => {
    return articles.filter((article) => {
      if (masterFamilyFilter && String(article.family_id ?? "") !== masterFamilyFilter) {
        return false;
      }
      if (masterStatusFilter === "active" && article.is_active !== 1) return false;
      if (masterStatusFilter === "inactive" && article.is_active === 1) return false;
      return true;
    });
  }, [articles, masterFamilyFilter, masterStatusFilter]);

  const switchProcurementView = useCallback((v: "list" | "kanban") => {
    setProcurementView(v);
    localStorage.setItem("inv-procurement-view", v);
  }, []);

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

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Package className={mfLayout.moduleHeaderIcon} aria-hidden />
          <h1 className={mfLayout.moduleTitle}>Inventory - Item Master & Stock</h1>
          <Badge variant="secondary">{articles.length} articles</Badge>
        </div>
        <div className={mfLayout.moduleHeaderActions}>
          {invTab === "master" ? (
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                size="sm"
                className="gap-1.5"
                onClick={() => {
                  resetArticleForm();
                  setCreateArticleOpen(true);
                }}
              >
                <Plus className="h-3.5 w-3.5" />
                Create article
              </Button>
            </PermissionGate>
          ) : null}
          {invTab === "topology" ? (
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                size="sm"
                className="gap-1.5"
                onClick={() => warehousePanelRef.current?.openCreateWarehouse()}
              >
                <Plus className="h-3.5 w-3.5" />
                Create warehouse
              </Button>
            </PermissionGate>
          ) : null}

          {invTab === "procurement" ? (
            <PermissionGate permission={P.INV_PROCURE}>
              <Button
                size="sm"
                className="gap-1.5"
                onClick={() => procurementPanelRef.current?.openCreateRequisition()}
              >
                <Plus className="h-3.5 w-3.5" />
                New requisition
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="gap-1.5"
                onClick={() => procurementPanelRef.current?.openCreatePo()}
              >
                <Plus className="h-3.5 w-3.5" />
                Create PO
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="gap-1.5"
                onClick={() => procurementPanelRef.current?.openReceiveGoods()}
              >
                Receive goods
              </Button>
              <Button
                size="sm"
                variant="outline"
                className="gap-1.5"
                onClick={() => procurementPanelRef.current?.openRepairable()}
              >
                New repairable
              </Button>
            </PermissionGate>
          ) : null}

          {invTab === "procurement" ? (
            <div className={mfLayout.viewToggleGroup}>
              <Button
                type="button"
                variant={procurementView === "list" ? "default" : "ghost"}
                size="sm"
                className={mfLayout.viewToggleButton}
                onClick={() => switchProcurementView("list")}
                title="List view"
              >
                <List className="h-3.5 w-3.5" />
              </Button>
              <Button
                type="button"
                variant={procurementView === "kanban" ? "default" : "ghost"}
                size="sm"
                className={mfLayout.viewToggleButton}
                onClick={() => switchProcurementView("kanban")}
                title="Kanban view"
              >
                <Columns3 className="h-3.5 w-3.5" />
              </Button>
            </div>
          ) : null}

          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              if (invTab === "procurement") {
                void procurementPanelRef.current?.reload();
                return;
              }
              void loadAll();
            }}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </Button>
        </div>
      </div>

      {error ? <div className="px-6 py-2 text-sm text-destructive">{error}</div> : null}

      <Tabs
        value={invTab}
        onValueChange={setInvTab}
        className="flex min-h-0 flex-1 flex-col px-6 py-4"
      >
        <TabsList className="w-fit">
          <TabsTrigger value="master">Item master</TabsTrigger>
          <TabsTrigger value="topology">Warehouses & locations</TabsTrigger>
          <TabsTrigger value="procurement">Procurement & repairables</TabsTrigger>
          <TabsTrigger value="controls">Controls & reconciliation</TabsTrigger>
        </TabsList>

        <TabsContent value="master" className="mt-4 min-h-0 flex-1 data-[state=inactive]:hidden">
          <div className={mfLayout.moduleWorkspaceSplit}>
            <div className="flex w-[55%] min-w-[400px] flex-col border-r border-surface-border">
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
                  onDeactivate={() => void softDeleteSelectedArticle()}
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
          />
        </TabsContent>
        <TabsContent value="controls" className="mt-4 space-y-4">
          <InventoryControlsPanel />
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
            <DialogTitle>Edit article</DialogTitle>
            <DialogDescription>Update catalog data and replenishment parameters.</DialogDescription>
          </DialogHeader>
          <PermissionGate permission={P.INV_MANAGE}>
            <ArticleEditorFields
              articleForm={articleForm}
              setArticleForm={setArticleForm}
              families={families}
              warehouses={warehouses}
              preferredWarehouseLocations={preferredWarehouseLocations}
              unitOptions={unitOptions}
              criticalityOptions={criticalityOptions}
              stockingTypeOptions={stockingTypeOptions}
              taxCategoryOptions={taxCategoryOptions}
              procurementCategoryOptions={procurementCategoryOptions}
            />
          </PermissionGate>
          <DialogFooter>
            <Button variant="outline" onClick={() => setEditingInDialog(false)}>
              Cancel
            </Button>
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                onClick={() => void saveEditedArticle()}
                disabled={saving || !isArticleFormValid}
              >
                Save
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
            <DialogTitle>New article</DialogTitle>
            <DialogDescription>
              Define catalog data and replenishment parameters. Reorder point triggers replenishment
              alerts; maximum stock is optional.
            </DialogDescription>
          </DialogHeader>
          <PermissionGate permission={P.INV_MANAGE}>
            <ArticleEditorFields
              articleForm={articleForm}
              setArticleForm={setArticleForm}
              families={families}
              warehouses={warehouses}
              preferredWarehouseLocations={preferredWarehouseLocations}
              unitOptions={unitOptions}
              criticalityOptions={criticalityOptions}
              stockingTypeOptions={stockingTypeOptions}
              taxCategoryOptions={taxCategoryOptions}
              procurementCategoryOptions={procurementCategoryOptions}
            />
          </PermissionGate>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => setCreateArticleOpen(false)}>
              Cancel
            </Button>
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                type="button"
                onClick={() => void createNewArticle()}
                disabled={saving || !isArticleFormValid}
              >
                <Plus className="mr-1 h-4 w-4" />
                Create article
              </Button>
            </PermissionGate>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
