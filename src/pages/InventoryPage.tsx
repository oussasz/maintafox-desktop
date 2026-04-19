import type { ColumnDef } from "@tanstack/react-table";
import { Columns3, Filter, List, Package, Plus, RefreshCw, Search, X } from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { ArticleEditorFields } from "@/components/inventory/ArticleEditorFields";
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
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { getLookupValues } from "@/services/lookup-service";
import { useInventoryStore } from "@/stores/inventory-store";
import type {
  InventoryArticle,
  InventoryArticleInput,
  InventoryStockBalance,
  LookupValueOption,
} from "@shared/ipc-types";

const EMPTY_ARTICLE_FORM: InventoryArticleInput = {
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
};

export function InventoryPage() {
  const families = useInventoryStore((s) => s.families);
  const warehouses = useInventoryStore((s) => s.warehouses);
  const locations = useInventoryStore((s) => s.locations);
  const articles = useInventoryStore((s) => s.articles);
  const balances = useInventoryStore((s) => s.balances);
  const selectedWarehouseId = useInventoryStore((s) => s.selectedWarehouseId);
  const loading = useInventoryStore((s) => s.loading);
  const saving = useInventoryStore((s) => s.saving);
  const error = useInventoryStore((s) => s.error);
  const loadAll = useInventoryStore((s) => s.loadAll);
  const setWarehouse = useInventoryStore((s) => s.setWarehouse);
  const setLowStockOnly = useInventoryStore((s) => s.setLowStockOnly);
  const setArticleSearch = useInventoryStore((s) => s.setArticleSearch);
  const createArticle = useInventoryStore((s) => s.createArticle);
  const updateArticle = useInventoryStore((s) => s.updateArticle);
  const adjustStock = useInventoryStore((s) => s.adjustStock);

  const [unitOptions, setUnitOptions] = useState<LookupValueOption[]>([]);
  const [criticalityOptions, setCriticalityOptions] = useState<LookupValueOption[]>([]);
  const [stockingTypeOptions, setStockingTypeOptions] = useState<LookupValueOption[]>([]);
  const [taxCategoryOptions, setTaxCategoryOptions] = useState<LookupValueOption[]>([]);
  const [procurementCategoryOptions, setProcurementCategoryOptions] = useState<LookupValueOption[]>(
    [],
  );
  const [articleForm, setArticleForm] = useState<InventoryArticleInput>(EMPTY_ARTICLE_FORM);
  const [selectedArticle, setSelectedArticle] = useState<InventoryArticle | null>(null);
  const [isDetailOpen, setDetailOpen] = useState(false);
  const [isEditingInDialog, setEditingInDialog] = useState(false);
  const [stockArticleId, setStockArticleId] = useState<number>(0);
  const [stockLocationId, setStockLocationId] = useState<number>(0);
  const [stockDelta, setStockDelta] = useState<number>(0);
  const [lowOnly, setLowOnly] = useState(false);

  const articleSearchStore = useInventoryStore((s) => s.articleSearch);
  const [invTab, setInvTab] = useState("master");
  const warehousePanelRef = useRef<WarehouseLocationPanelHandle>(null);
  const procurementPanelRef = useRef<ProcurementRepairablePanelHandle>(null);
  const [showInvFilters, setShowInvFilters] = useState(
    () => localStorage.getItem("inv-show-filters") !== "0",
  );
  const [procurementView, setProcurementView] = useState<"list" | "kanban">(
    () => (localStorage.getItem("inv-procurement-view") as "list" | "kanban") || "list",
  );
  const [masterSearchInput, setMasterSearchInput] = useState("");
  const masterSearchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [topologySearchFilter, setTopologySearchFilter] = useState("");
  const [stockBalanceSearch, setStockBalanceSearch] = useState("");
  const [isCreateArticleOpen, setCreateArticleOpen] = useState(false);

  useEffect(() => {
    void loadAll();
  }, [loadAll]);

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

  const stockColumns: ColumnDef<InventoryStockBalance>[] = useMemo(
    () => [
      { accessorKey: "article_code", header: "Article" },
      { accessorKey: "warehouse_code", header: "Warehouse" },
      { accessorKey: "location_code", header: "Location" },
      { accessorKey: "on_hand_qty", header: "On-hand" },
      { accessorKey: "reserved_qty", header: "Reserved" },
      { accessorKey: "available_qty", header: "Available" },
      { accessorKey: "updated_at", header: "Updated" },
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
    setSelectedArticle(article);
    setEditingInDialog(false);
    setDetailOpen(true);
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
      `Delete article ${selectedArticle.article_code}? It will be deactivated and hidden from active operations.`,
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
    setDetailOpen(false);
    setSelectedArticle(null);
  };

  const createNewArticle = async () => {
    await createArticle(articleForm);
    resetArticleForm();
    setCreateArticleOpen(false);
  };

  const handleMasterSearchChange = useCallback(
    (e: ChangeEvent<HTMLInputElement>) => {
      const val = e.target.value;
      setMasterSearchInput(val);
      if (masterSearchTimerRef.current) clearTimeout(masterSearchTimerRef.current);
      masterSearchTimerRef.current = setTimeout(() => {
        void setArticleSearch(val.trim());
      }, 300);
    },
    [setArticleSearch],
  );

  const clearMasterSearch = useCallback(() => {
    setMasterSearchInput("");
    void setArticleSearch("");
  }, [setArticleSearch]);

  const switchProcurementView = useCallback((v: "list" | "kanban") => {
    setProcurementView(v);
    localStorage.setItem("inv-procurement-view", v);
  }, []);

  const toggleInvFilters = useCallback(() => {
    setShowInvFilters((prev) => {
      const next = !prev;
      localStorage.setItem("inv-show-filters", next ? "1" : "0");
      return next;
    });
  }, []);

  const articleHistory = useMemo(() => {
    if (!selectedArticle) return [];
    return balances
      .filter((balance) => balance.article_id === selectedArticle.id)
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  }, [balances, selectedArticle]);

  const filteredStockBalances = useMemo(() => {
    const q = stockBalanceSearch.trim().toLowerCase();
    if (!q) return balances;
    return balances.filter(
      (b) =>
        b.article_code.toLowerCase().includes(q) ||
        b.article_name.toLowerCase().includes(q) ||
        b.warehouse_code.toLowerCase().includes(q) ||
        b.location_code.toLowerCase().includes(q),
    );
  }, [balances, stockBalanceSearch]);

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
            <PermissionGate permission="inv.manage">
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
            <PermissionGate permission="inv.manage">
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
            <PermissionGate permission="inv.procure">
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
            type="button"
            variant="outline"
            size="sm"
            onClick={toggleInvFilters}
            title="Filters"
            className="gap-1.5"
          >
            <Filter className="h-3.5 w-3.5" />
          </Button>

          <Button
            variant="outline"
            size="sm"
            onClick={() => void loadAll()}
            disabled={loading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? "animate-spin" : ""}`} />
          </Button>
        </div>
      </div>

      {showInvFilters ? (
        <div className={mfLayout.moduleFilterBar}>
          {invTab === "master" ? (
            <div className="relative max-w-sm flex-1">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
              <Input
                className="h-8 pl-9 text-sm"
                placeholder="Search article code or name"
                value={masterSearchInput}
                onChange={handleMasterSearchChange}
              />
              {masterSearchInput ? (
                <button
                  type="button"
                  className="absolute right-2 top-2 text-text-muted hover:text-text-primary"
                  onClick={clearMasterSearch}
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              ) : null}
            </div>
          ) : null}

          {invTab === "topology" ? (
            <div className="relative max-w-sm flex-1">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
              <Input
                className="h-8 pl-9 text-sm"
                placeholder="Search warehouse code or name"
                value={topologySearchFilter}
                onChange={(e) => setTopologySearchFilter(e.target.value)}
              />
              {topologySearchFilter ? (
                <button
                  type="button"
                  className="absolute right-2 top-2 text-text-muted hover:text-text-primary"
                  onClick={() => setTopologySearchFilter("")}
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              ) : null}
            </div>
          ) : null}

          {invTab === "stock" ? (
            <div className="flex w-full min-w-0 flex-nowrap items-center gap-3 overflow-x-auto pb-0.5 sm:overflow-visible sm:pb-0">
              <div className="relative min-w-0 flex-1 basis-0">
                <Search className="pointer-events-none absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted" />
                <Input
                  className="h-8 pl-9 pr-8 text-sm"
                  placeholder="Filter balances (article, warehouse, location…)"
                  value={stockBalanceSearch}
                  onChange={(e) => setStockBalanceSearch(e.target.value)}
                />
                {stockBalanceSearch ? (
                  <button
                    type="button"
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-primary"
                    onClick={() => setStockBalanceSearch("")}
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                ) : null}
              </div>

              <div className="flex shrink-0 items-center gap-2">
                <span className="hidden text-xs text-text-muted sm:inline whitespace-nowrap">
                  Warehouse
                </span>
                <Select
                  value={String(selectedWarehouseId ?? "__all__")}
                  onValueChange={(v) => void setWarehouse(v === "__all__" ? null : Number(v))}
                >
                  <SelectTrigger className="h-8 w-[min(220px,42vw)] text-sm" aria-label="Warehouse">
                    <SelectValue placeholder="Warehouse" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__all__">All warehouses</SelectItem>
                    {warehouses.map((w) => (
                      <SelectItem key={w.id} value={String(w.id)}>
                        {w.code} - {w.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <label
                htmlFor="low-only-inv"
                className="flex shrink-0 cursor-pointer select-none items-center gap-2 whitespace-nowrap rounded-md border border-surface-border bg-surface-1 px-2.5 py-1 text-sm text-text-primary hover:bg-surface-2"
              >
                <Checkbox
                  id="low-only-inv"
                  checked={lowOnly}
                  onCheckedChange={(checked) => {
                    const next = checked === true;
                    setLowOnly(next);
                    void setLowStockOnly(next);
                  }}
                  className="shrink-0"
                />
                <span>Low stock only</span>
              </label>
            </div>
          ) : null}
        </div>
      ) : null}

      {error ? <div className="px-6 py-2 text-sm text-destructive">{error}</div> : null}

      <Tabs
        value={invTab}
        onValueChange={setInvTab}
        className="flex min-h-0 flex-1 flex-col px-6 py-4"
      >
        <TabsList className="w-fit">
          <TabsTrigger value="master">Item master</TabsTrigger>
          <TabsTrigger value="topology">Warehouses & locations</TabsTrigger>
          <TabsTrigger value="stock">Stock balances</TabsTrigger>
          <TabsTrigger value="procurement">Procurement & repairables</TabsTrigger>
          <TabsTrigger value="controls">Controls & reconciliation</TabsTrigger>
        </TabsList>

        <TabsContent value="master" className="mt-4 min-h-0 flex-1 space-y-4">
          <div className="overflow-auto p-1">
            <DataTable
              columns={articleColumns}
              data={articles}
              isLoading={loading}
              searchable={false}
              onRowClick={openArticleDetails}
            />
          </div>
        </TabsContent>

        <TabsContent value="topology" className="mt-4 space-y-4">
          <WarehouseLocationPanel ref={warehousePanelRef} searchFilter={topologySearchFilter} />
        </TabsContent>

        <TabsContent value="stock" className="mt-4 space-y-4">
          <PermissionGate permission="inv.manage">
            <div className="rounded-md border p-4">
              <h3 className="mb-3 text-sm font-semibold">Stock adjustment</h3>
              <div className="grid gap-2 md:grid-cols-4">
                <Select
                  value={String(stockArticleId)}
                  onValueChange={(v) => setStockArticleId(Number(v))}
                >
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

                <Select
                  value={String(stockLocationId)}
                  onValueChange={(v) => setStockLocationId(Number(v))}
                >
                  <SelectTrigger>
                    <SelectValue placeholder="Location" />
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

                <Input
                  type="number"
                  step="0.01"
                  value={stockDelta}
                  onChange={(e) => setStockDelta(Number(e.target.value || 0))}
                  placeholder="Delta qty (+/-)"
                />

                <Button
                  onClick={() =>
                    void adjustStock({
                      article_id: stockArticleId,
                      location_id: stockLocationId,
                      delta_qty: stockDelta,
                    })
                  }
                  disabled={
                    saving || stockArticleId <= 0 || stockLocationId <= 0 || stockDelta === 0
                  }
                >
                  Post adjustment
                </Button>
              </div>
            </div>
          </PermissionGate>

          <Separator />
          <DataTable
            columns={stockColumns}
            data={filteredStockBalances}
            isLoading={loading}
            searchable={false}
          />
        </TabsContent>
        <TabsContent value="procurement" className="mt-4 space-y-4">
          <ProcurementRepairablePanel
            ref={procurementPanelRef}
            viewMode={procurementView}
            onViewModeChange={switchProcurementView}
          />
        </TabsContent>
        <TabsContent value="controls" className="mt-4 space-y-4">
          <InventoryControlsPanel />
        </TabsContent>
      </Tabs>

      <Dialog
        open={isDetailOpen}
        onOpenChange={(open) => {
          setDetailOpen(open);
          if (!open) setEditingInDialog(false);
        }}
      >
        <DialogContent className="max-h-[85vh] max-w-3xl overflow-y-auto">
          <DialogHeader>
            <DialogTitle>
              {selectedArticle
                ? `${selectedArticle.article_code} - ${selectedArticle.article_name}`
                : "Article details"}
            </DialogTitle>
            <DialogDescription>
              Full article details and stock history across locations.
            </DialogDescription>
          </DialogHeader>

          {selectedArticle ? (
            <div className="space-y-4">
              {!isEditingInDialog ? (
                <div className="grid grid-cols-2 gap-3 rounded-md border p-3 text-sm">
                  <div>
                    <span className="text-text-muted">Family:</span>{" "}
                    {selectedArticle.family_name ?? "—"}
                  </div>
                  <div>
                    <span className="text-text-muted">Unit:</span> {selectedArticle.unit_label}
                  </div>
                  <div>
                    <span className="text-text-muted">Criticality:</span>{" "}
                    {selectedArticle.criticality_label ?? "—"}
                  </div>
                  <div>
                    <span className="text-text-muted">Status:</span>{" "}
                    {selectedArticle.is_active === 1 ? "Active" : "Inactive"}
                  </div>
                  <div>
                    <span className="text-text-muted">Stocking type:</span>{" "}
                    {selectedArticle.stocking_type_label}
                  </div>
                  <div>
                    <span className="text-text-muted">Tax category:</span>{" "}
                    {selectedArticle.tax_category_label}
                  </div>
                  <div>
                    <span className="text-text-muted">Procurement category:</span>{" "}
                    {selectedArticle.procurement_category_label ?? "—"}
                  </div>
                  <div>
                    <span className="text-text-muted">Preferred warehouse:</span>{" "}
                    {selectedArticle.preferred_warehouse_code ?? "—"}
                  </div>
                  <div>
                    <span className="text-text-muted">Preferred location:</span>{" "}
                    {selectedArticle.preferred_location_code ?? "—"}
                  </div>
                  <div>
                    <span className="text-text-muted">Min stock:</span> {selectedArticle.min_stock}
                  </div>
                  <div>
                    <span className="text-text-muted">Reorder point:</span>{" "}
                    {selectedArticle.reorder_point}
                  </div>
                  <div>
                    <span className="text-text-muted">Safety stock:</span>{" "}
                    {selectedArticle.safety_stock}
                  </div>
                  <div>
                    <span className="text-text-muted">Created at:</span>{" "}
                    {selectedArticle.created_at}
                  </div>
                  <div>
                    <span className="text-text-muted">Updated at:</span>{" "}
                    {selectedArticle.updated_at}
                  </div>
                </div>
              ) : (
                <div className="rounded-md border p-3">
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
                </div>
              )}

              <div className="rounded-md border p-3">
                <h4 className="mb-2 text-sm font-semibold">Historique</h4>
                {articleHistory.length === 0 ? (
                  <p className="text-sm text-text-muted">
                    No stock history entries yet for this article.
                  </p>
                ) : (
                  <div className="space-y-2">
                    {articleHistory.map((entry) => (
                      <div key={entry.id} className="rounded border p-2 text-sm">
                        <div className="font-medium">
                          {entry.warehouse_code}/{entry.location_code}
                        </div>
                        <div className="text-text-muted">
                          On-hand: {entry.on_hand_qty} | Reserved: {entry.reserved_qty} | Available:{" "}
                          {entry.available_qty}
                        </div>
                        <div className="text-xs text-text-muted">Updated: {entry.updated_at}</div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          ) : null}

          <DialogFooter>
            <Button variant="outline" onClick={() => setDetailOpen(false)}>
              Close
            </Button>
            <PermissionGate permission="inv.manage">
              {!isEditingInDialog ? (
                <>
                  <Button variant="outline" onClick={beginEditSelectedArticle}>
                    Edit
                  </Button>
                  <Button
                    variant="destructive"
                    onClick={() => void softDeleteSelectedArticle()}
                    disabled={saving}
                  >
                    Delete
                  </Button>
                </>
              ) : (
                <>
                  <Button variant="outline" onClick={() => setEditingInDialog(false)}>
                    Cancel
                  </Button>
                  <Button onClick={() => void saveEditedArticle()} disabled={saving}>
                    Save
                  </Button>
                </>
              )}
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
          <PermissionGate permission="inv.manage">
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
            <PermissionGate permission="inv.manage">
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
