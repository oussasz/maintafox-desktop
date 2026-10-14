import type { ColumnDef } from "@tanstack/react-table";
import { MapPin, Warehouse as WarehouseIcon } from "lucide-react";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import { DetailFieldRow, DetailSectionCard, EntityDetailHeader } from "@/components/detail";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { mfLayout } from "@/design-system/tokens";
import {
  createInventoryStockLocation,
  createInventoryWarehouse,
  listInventoryLocations,
  updateInventoryStockLocation,
  updateInventoryWarehouse,
} from "@/services/inventory-service";
import { useInventoryStore } from "@/stores/inventory-store";
import type { StockLocation, Warehouse } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export type WarehouseLocationPanelHandle = {
  openCreateWarehouse: () => void;
};

export const WarehouseLocationPanel = forwardRef<WarehouseLocationPanelHandle>(
  function WarehouseLocationPanel(_props, ref) {
    const { t } = useTranslation("inventory");
    const { t: tc } = useTranslation("common");
    const warehouses = useInventoryStore((s) => s.warehouses);
    const loadAll = useInventoryStore((s) => s.loadAll);
    const saving = useInventoryStore((s) => s.saving);

    const [searchInput, setSearchInput] = useState("");
    const [searchFilter, setSearchFilter] = useState("");
    const [selectedWarehouse, setSelectedWarehouse] = useState<Warehouse | null>(null);

    const [createOpen, setCreateOpen] = useState(false);
    const [createCode, setCreateCode] = useState("");
    const [createName, setCreateName] = useState("");

    const [whName, setWhName] = useState("");
    const [whActive, setWhActive] = useState(true);

    const [locations, setLocations] = useState<StockLocation[]>([]);
    const [locSaving, setLocSaving] = useState(false);
    /** Failure to load the selected warehouse's locations (save failures go to the toast). */
    const [locLoadError, setLocLoadError] = useState<unknown>(null);
    const [whSaving, setWhSaving] = useState(false);

    const [newLocCode, setNewLocCode] = useState("");
    const [newLocName, setNewLocName] = useState("");
    const [newLocDefault, setNewLocDefault] = useState(false);

    const [editLoc, setEditLoc] = useState<StockLocation | null>(null);
    const [editLocCode, setEditLocCode] = useState("");
    const [editLocName, setEditLocName] = useState("");
    const [editLocDefault, setEditLocDefault] = useState(false);
    const [editLocActive, setEditLocActive] = useState(true);

    const reloadLocations = useCallback(async (warehouseId: number) => {
      try {
        const rows = await listInventoryLocations(warehouseId);
        setLocations(rows);
        setLocLoadError(null);
      } catch (err) {
        setLocLoadError(err);
      }
    }, []);

    useEffect(() => {
      setSelectedWarehouse((prev) => {
        if (!prev) return null;
        return warehouses.find((w) => w.id === prev.id) ?? null;
      });
    }, [warehouses]);

    useEffect(() => {
      if (!selectedWarehouse) {
        setLocations([]);
        setWhName("");
        setWhActive(true);
        setEditLoc(null);
        setLocLoadError(null);
        return;
      }
      const warehouseId = selectedWarehouse.id;
      setWhName(selectedWarehouse.name);
      setWhActive(selectedWarehouse.is_active === 1);
      setEditLoc(null);
      void reloadLocations(warehouseId);
      // Sync form when selection changes, not on every warehouse list refresh mid-edit.
      // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional: selectedWarehouse.id only
    }, [selectedWarehouse?.id, reloadLocations]);

    useImperativeHandle(ref, () => ({
      openCreateWarehouse: () => {
        setCreateCode("");
        setCreateName("");
        setCreateOpen(true);
      },
    }));

    const filteredWarehouses = useMemo(() => {
      const q = searchFilter.trim().toLowerCase();
      if (!q) return warehouses;
      return warehouses.filter(
        (w) => w.code.toLowerCase().includes(q) || w.name.toLowerCase().includes(q),
      );
    }, [warehouses, searchFilter]);

    const selectWarehouse = (w: Warehouse) => {
      setSelectedWarehouse((prev) => (prev?.id === w.id ? null : w));
    };

    // Save failures are reported by the global IPC toast; forms stay open with the user's input.
    const handleCreateWarehouse = async () => {
      if (!createCode.trim() || !createName.trim()) return;
      setWhSaving(true);
      try {
        const created = await createInventoryWarehouse({
          code: createCode.trim(),
          name: createName.trim(),
        });
        await loadAll();
        setCreateOpen(false);
        setSelectedWarehouse(created);
      } catch {
        // reported by the global IPC toast
      } finally {
        setWhSaving(false);
      }
    };

    const handleSaveWarehouse = async () => {
      if (!selectedWarehouse || !whName.trim()) return;
      setWhSaving(true);
      try {
        await updateInventoryWarehouse(selectedWarehouse.id, {
          name: whName.trim(),
          is_active: whActive,
        });
        await loadAll();
        const refreshed = useInventoryStore
          .getState()
          .warehouses.find((x) => x.id === selectedWarehouse.id);
        if (refreshed) setSelectedWarehouse(refreshed);
      } catch {
        // reported by the global IPC toast
      } finally {
        setWhSaving(false);
      }
    };

    const handleAddLocation = async () => {
      if (!selectedWarehouse || !newLocCode.trim() || !newLocName.trim()) return;
      setLocSaving(true);
      try {
        await createInventoryStockLocation({
          warehouse_id: selectedWarehouse.id,
          code: newLocCode.trim(),
          name: newLocName.trim(),
          is_default: newLocDefault,
        });
        setNewLocCode("");
        setNewLocName("");
        setNewLocDefault(false);
        await reloadLocations(selectedWarehouse.id);
        await loadAll();
      } catch {
        // reported by the global IPC toast
      } finally {
        setLocSaving(false);
      }
    };

    const beginEditLocation = (loc: StockLocation) => {
      setEditLoc(loc);
      setEditLocCode(loc.code);
      setEditLocName(loc.name);
      setEditLocDefault(loc.is_default === 1);
      setEditLocActive(loc.is_active === 1);
    };

    const cancelEditLocation = () => {
      setEditLoc(null);
    };

    const saveEditLocation = async () => {
      if (!editLoc || !selectedWarehouse || !editLocCode.trim() || !editLocName.trim()) return;
      setLocSaving(true);
      try {
        await updateInventoryStockLocation(editLoc.id, editLoc.row_version, {
          code: editLocCode.trim(),
          name: editLocName.trim(),
          is_default: editLocDefault,
          is_active: editLocActive,
        });
        cancelEditLocation();
        await reloadLocations(selectedWarehouse.id);
        await loadAll();
      } catch {
        // reported by the global IPC toast
      } finally {
        setLocSaving(false);
      }
    };

    const resetTopologyFilters = useCallback(() => {
      setSearchInput("");
      setSearchFilter("");
    }, []);

    const whColumns: ColumnDef<Warehouse>[] = useMemo(
      () => [
        { accessorKey: "code", header: t("topology.columns.code") },
        { accessorKey: "name", header: t("topology.columns.name") },
        {
          accessorKey: "is_active",
          header: t("topology.columns.status"),
          cell: ({ row }) =>
            row.original.is_active === 1 ? (
              <StatusChip label={tc("status.active")} tone="success" dot />
            ) : (
              <StatusChip label={tc("status.inactive")} tone="neutral" dot />
            ),
        },
      ],
      [t, tc],
    );

    const locColumns: ColumnDef<StockLocation>[] = useMemo(
      () => [
        { accessorKey: "code", header: t("topology.columns.code") },
        { accessorKey: "name", header: t("topology.columns.name") },
        {
          accessorKey: "is_default",
          header: t("topology.columns.default"),
          cell: ({ row }) => (row.original.is_default === 1 ? tc("action.yes") : tc("action.no")),
        },
        {
          accessorKey: "is_active",
          header: t("topology.columns.active"),
          cell: ({ row }) =>
            row.original.is_active === 1 ? tc("status.active") : tc("status.inactive"),
        },
        {
          id: "actions",
          header: "",
          cell: ({ row }) => (
            <PermissionGate permission={P.INV_MANAGE}>
              <Button
                type="button"
                variant="outline"
                className="h-9"
                aria-label={t("topology.actions.editLocationNamed", { code: row.original.code })}
                onClick={(e) => {
                  e.stopPropagation();
                  beginEditLocation(row.original);
                }}
              >
                {tc("action.edit")}
              </Button>
            </PermissionGate>
          ),
        },
      ],
      [t, tc],
    );

    return (
      <div className={mfLayout.moduleWorkspaceSplit}>
        <div className="flex w-[55%] min-w-[400px] flex-col border-r border-surface-border">
          <SmartFilterBar
            searchPlaceholder={t("filters.searchWarehouse")}
            searchValue={searchInput}
            onSearchInputChange={setSearchInput}
            onSearchChange={(q) => setSearchFilter(q.trim())}
            filters={[]}
            resultCount={filteredWarehouses.length}
            onReset={resetTopologyFilters}
          />
          <div className="min-h-0 flex-1 overflow-auto p-1">
            <DataTable
              columns={whColumns}
              data={filteredWarehouses}
              isLoading={false}
              searchable={false}
              onRowClick={selectWarehouse}
              isRowSelected={(row) => row.id === selectedWarehouse?.id}
            />
          </div>
        </div>

        <div className="min-w-[300px] flex-1 overflow-hidden">
          {selectedWarehouse ? (
            <div className="flex h-full flex-col space-y-4 overflow-auto p-4">
              <EntityDetailHeader
                code={selectedWarehouse.code}
                designation={selectedWarehouse.name}
                statusSlot={
                  <StatusChip
                    label={
                      selectedWarehouse.is_active === 1
                        ? tc("status.active")
                        : tc("status.inactive")
                    }
                    tone={selectedWarehouse.is_active === 1 ? "success" : "neutral"}
                    dot
                  />
                }
              />

              <DetailSectionCard title={t("topology.sections.warehouse")} icon={WarehouseIcon}>
                <DetailFieldRow
                  label={t("topology.fields.code")}
                  value={selectedWarehouse.code}
                  mono
                />
                <PermissionGate permission={P.INV_MANAGE}>
                  <div className="space-y-3 pt-1">
                    <div className="space-y-1">
                      <Label htmlFor="wh-name">{t("topology.fields.name")}</Label>
                      <Input
                        id="wh-name"
                        value={whName}
                        onChange={(e) => setWhName(e.target.value)}
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <Switch id="wh-active" checked={whActive} onCheckedChange={setWhActive} />
                      <Label htmlFor="wh-active">{t("topology.fields.active")}</Label>
                    </div>
                    <Button
                      type="button"
                      className="h-10"
                      onClick={() => void handleSaveWarehouse()}
                      disabled={saving || whSaving || !whName.trim()}
                    >
                      {t("topology.actions.saveWarehouse")}
                    </Button>
                  </div>
                </PermissionGate>
              </DetailSectionCard>

              <DetailSectionCard title={t("topology.sections.locations")} icon={MapPin}>
                <PermissionGate permission={P.INV_MANAGE}>
                  <div className="mb-3 space-y-3 rounded-md border border-surface-border p-3">
                    <div className="grid gap-2 md:grid-cols-3">
                      <div className="space-y-1">
                        <Label htmlFor="new-loc-code">{t("topology.fields.locationCode")}</Label>
                        <Input
                          id="new-loc-code"
                          value={newLocCode}
                          onChange={(e) => setNewLocCode(e.target.value)}
                          placeholder={t("topology.placeholders.locationCode")}
                        />
                      </div>
                      <div className="space-y-1">
                        <Label htmlFor="new-loc-name">{t("topology.fields.locationName")}</Label>
                        <Input
                          id="new-loc-name"
                          value={newLocName}
                          onChange={(e) => setNewLocName(e.target.value)}
                          placeholder={t("topology.placeholders.locationName")}
                        />
                      </div>
                      <div className="flex items-end gap-2 pb-0.5">
                        <div className="flex items-center gap-2">
                          <Switch
                            id="new-loc-def"
                            checked={newLocDefault}
                            onCheckedChange={setNewLocDefault}
                          />
                          <Label htmlFor="new-loc-def">{t("topology.fields.defaultBin")}</Label>
                        </div>
                      </div>
                    </div>
                    <Button
                      type="button"
                      variant="secondary"
                      className="h-10"
                      disabled={locSaving || !newLocCode.trim() || !newLocName.trim()}
                      onClick={() => void handleAddLocation()}
                    >
                      {t("topology.actions.addLocation")}
                    </Button>
                  </div>
                </PermissionGate>

                {locLoadError ? (
                  <ErrorState
                    error={locLoadError}
                    onRetry={() => void reloadLocations(selectedWarehouse.id)}
                  />
                ) : locations.length === 0 ? (
                  <p className="text-xs text-text-muted">{t("topology.emptyLocations")}</p>
                ) : (
                  <DataTable
                    columns={locColumns}
                    data={locations}
                    searchable={false}
                    isLoading={false}
                  />
                )}

                {editLoc ? (
                  <div className="mt-3 space-y-2 rounded-md border bg-muted/30 p-3">
                    <div className="text-sm font-medium">
                      {t("topology.actions.editLocationNamed", { code: editLoc.code })}
                    </div>
                    <div className="grid gap-2 md:grid-cols-2">
                      <div className="space-y-1">
                        <Label htmlFor="el-code">{t("topology.fields.locationCode")}</Label>
                        <Input
                          id="el-code"
                          value={editLocCode}
                          onChange={(e) => setEditLocCode(e.target.value)}
                        />
                      </div>
                      <div className="space-y-1">
                        <Label htmlFor="el-name">{t("topology.fields.locationName")}</Label>
                        <Input
                          id="el-name"
                          value={editLocName}
                          onChange={(e) => setEditLocName(e.target.value)}
                        />
                      </div>
                      <div className="flex items-center gap-2">
                        <Switch
                          id="el-def"
                          checked={editLocDefault}
                          onCheckedChange={setEditLocDefault}
                        />
                        <Label htmlFor="el-def">{t("topology.fields.defaultBin")}</Label>
                      </div>
                      <div className="flex items-center gap-2">
                        <Switch
                          id="el-act"
                          checked={editLocActive}
                          onCheckedChange={setEditLocActive}
                        />
                        <Label htmlFor="el-act">{t("topology.fields.active")}</Label>
                      </div>
                    </div>
                    <div className="flex gap-2">
                      <Button
                        type="button"
                        className="h-10"
                        disabled={locSaving || !editLocCode.trim() || !editLocName.trim()}
                        onClick={() => void saveEditLocation()}
                      >
                        {t("topology.actions.saveLocation")}
                      </Button>
                      <Button
                        type="button"
                        variant="outline"
                        className="h-10"
                        onClick={cancelEditLocation}
                      >
                        {tc("action.cancel")}
                      </Button>
                    </div>
                  </div>
                ) : null}
              </DetailSectionCard>
            </div>
          ) : (
            <div className="flex h-full items-center justify-center p-6">
              <p className="text-sm text-text-muted">{t("topology.noSelection")}</p>
            </div>
          )}
        </div>

        <Dialog
          open={createOpen}
          onOpenChange={(o) => {
            setCreateOpen(o);
            if (!o) {
              setCreateCode("");
              setCreateName("");
            }
          }}
        >
          <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>{t("topology.createTitle")}</DialogTitle>
            </DialogHeader>
            <PermissionGate permission={P.INV_MANAGE}>
              <div className="grid gap-3">
                <div className="space-y-1">
                  <Label htmlFor="create-wh-code">{t("topology.fields.code")}</Label>
                  <Input
                    id="create-wh-code"
                    value={createCode}
                    onChange={(e) => setCreateCode(e.target.value)}
                    placeholder={t("topology.placeholders.warehouseCode")}
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="create-wh-name">{t("topology.fields.name")}</Label>
                  <Input
                    id="create-wh-name"
                    value={createName}
                    onChange={(e) => setCreateName(e.target.value)}
                    placeholder={t("topology.placeholders.warehouseName")}
                  />
                </div>
              </div>
            </PermissionGate>
            <DialogFooter className="gap-2">
              <Button
                type="button"
                variant="outline"
                className="h-11"
                onClick={() => setCreateOpen(false)}
              >
                {tc("action.cancel")}
              </Button>
              <PermissionGate permission={P.INV_MANAGE}>
                <Button
                  type="button"
                  className="h-11"
                  disabled={saving || whSaving || !createCode.trim() || !createName.trim()}
                  onClick={() => void handleCreateWarehouse()}
                >
                  {t("topology.actions.createWarehouse")}
                </Button>
              </PermissionGate>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    );
  },
);
