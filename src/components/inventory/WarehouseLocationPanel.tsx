import type { ColumnDef } from "@tanstack/react-table";
import { MapPin, Warehouse as WarehouseIcon } from "lucide-react";
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useState,
} from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
import {
  DetailFieldRow,
  DetailSectionCard,
  EntityDetailHeader,
} from "@/components/detail";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
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
import { toErrorMessage } from "@/utils/errors";
import type { StockLocation, Warehouse } from "@shared/ipc-types";

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
    const [locError, setLocError] = useState<string | null>(null);

    const [newLocCode, setNewLocCode] = useState("");
    const [newLocName, setNewLocName] = useState("");
    const [newLocDefault, setNewLocDefault] = useState(false);

    const [editLoc, setEditLoc] = useState<StockLocation | null>(null);
    const [editLocCode, setEditLocCode] = useState("");
    const [editLocName, setEditLocName] = useState("");
    const [editLocDefault, setEditLocDefault] = useState(false);
    const [editLocActive, setEditLocActive] = useState(true);

    const reloadLocations = useCallback(async (warehouseId: number) => {
      const rows = await listInventoryLocations(warehouseId);
      setLocations(rows);
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
        setLocError(null);
        return;
      }
      const warehouseId = selectedWarehouse.id;
      setWhName(selectedWarehouse.name);
      setWhActive(selectedWarehouse.is_active === 1);
      setEditLoc(null);
      setLocError(null);
      void reloadLocations(warehouseId).catch(() => setLocations([]));
      // Sync form when selection changes, not on every warehouse list refresh mid-edit.
      // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional: selectedWarehouse.id only
    }, [selectedWarehouse?.id, reloadLocations]);

    useImperativeHandle(ref, () => ({
      openCreateWarehouse: () => {
        setCreateCode("");
        setCreateName("");
        setLocError(null);
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

    const handleCreateWarehouse = async () => {
      if (!createCode.trim() || !createName.trim()) return;
      try {
        setLocError(null);
        const created = await createInventoryWarehouse({
          code: createCode.trim(),
          name: createName.trim(),
        });
        await loadAll();
        setCreateOpen(false);
        setSelectedWarehouse(created);
      } catch (err) {
        setLocError(toErrorMessage(err));
      }
    };

    const handleSaveWarehouse = async () => {
      if (!selectedWarehouse || !whName.trim()) return;
      try {
        setLocError(null);
        await updateInventoryWarehouse(selectedWarehouse.id, {
          name: whName.trim(),
          is_active: whActive,
        });
        await loadAll();
        const refreshed = useInventoryStore
          .getState()
          .warehouses.find((x) => x.id === selectedWarehouse.id);
        if (refreshed) setSelectedWarehouse(refreshed);
      } catch (err) {
        setLocError(toErrorMessage(err));
      }
    };

    const handleAddLocation = async () => {
      if (!selectedWarehouse || !newLocCode.trim() || !newLocName.trim()) return;
      setLocSaving(true);
      setLocError(null);
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
      } catch (err) {
        setLocError(toErrorMessage(err));
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
      setLocError(null);
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
      } catch (err) {
        setLocError(toErrorMessage(err));
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
              <Badge variant="secondary">{tc("status.active")}</Badge>
            ) : (
              <Badge>{tc("status.inactive")}</Badge>
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
          cell: ({ row }) =>
            row.original.is_default === 1 ? tc("action.yes") : tc("action.no"),
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
            <PermissionGate permission="inv.manage">
              <Button
                type="button"
                variant="outline"
                size="sm"
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
              {locError && !createOpen ? (
                <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2 text-sm">
                  {locError}
                </div>
              ) : null}

              <EntityDetailHeader
                code={selectedWarehouse.code}
                designation={selectedWarehouse.name}
                statusSlot={
                  <Badge variant={selectedWarehouse.is_active === 1 ? "secondary" : "outline"}>
                    {selectedWarehouse.is_active === 1
                      ? tc("status.active")
                      : tc("status.inactive")}
                  </Badge>
                }
              />

              <DetailSectionCard title={t("topology.sections.warehouse")} icon={WarehouseIcon}>
                <DetailFieldRow
                  label={t("topology.fields.code")}
                  value={selectedWarehouse.code}
                  mono
                />
                <PermissionGate permission="inv.manage">
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
                      size="sm"
                      onClick={() => void handleSaveWarehouse()}
                      disabled={saving || !whName.trim()}
                    >
                      {t("topology.actions.saveWarehouse")}
                    </Button>
                  </div>
                </PermissionGate>
              </DetailSectionCard>

              <DetailSectionCard title={t("topology.sections.locations")} icon={MapPin}>
                <PermissionGate permission="inv.manage">
                  <div className="mb-3 space-y-3 rounded-md border border-surface-border p-3">
                    <div className="grid gap-2 md:grid-cols-3">
                      <div className="space-y-1">
                        <Label htmlFor="new-loc-code">{t("topology.fields.locationCode")}</Label>
                        <Input
                          id="new-loc-code"
                          value={newLocCode}
                          onChange={(e) => setNewLocCode(e.target.value)}
                          placeholder="BIN-A1"
                        />
                      </div>
                      <div className="space-y-1">
                        <Label htmlFor="new-loc-name">{t("topology.fields.locationName")}</Label>
                        <Input
                          id="new-loc-name"
                          value={newLocName}
                          onChange={(e) => setNewLocName(e.target.value)}
                          placeholder="Shelf A1"
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
                      size="sm"
                      variant="secondary"
                      disabled={locSaving || !newLocCode.trim() || !newLocName.trim()}
                      onClick={() => void handleAddLocation()}
                    >
                      {t("topology.actions.addLocation")}
                    </Button>
                  </div>
                </PermissionGate>

                {locations.length === 0 ? (
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
                      {t("topology.actions.editLocation")} {editLoc.code}
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
                        size="sm"
                        disabled={locSaving}
                        onClick={() => void saveEditLocation()}
                      >
                        {t("topology.actions.saveLocation")}
                      </Button>
                      <Button type="button" size="sm" variant="outline" onClick={cancelEditLocation}>
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
              setLocError(null);
            }
          }}
        >
          <DialogContent className="max-w-md" onPointerDownOutside={(e) => e.preventDefault()}>
            <DialogHeader>
              <DialogTitle>{t("topology.createTitle")}</DialogTitle>
            </DialogHeader>
            {locError && createOpen ? (
              <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2 text-sm">
                {locError}
              </div>
            ) : null}
            <PermissionGate permission="inv.manage">
              <div className="grid gap-3">
                <div className="space-y-1">
                  <Label htmlFor="create-wh-code">{t("topology.fields.code")}</Label>
                  <Input
                    id="create-wh-code"
                    value={createCode}
                    onChange={(e) => setCreateCode(e.target.value)}
                    placeholder="e.g. WH01"
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="create-wh-name">{t("topology.fields.name")}</Label>
                  <Input
                    id="create-wh-name"
                    value={createName}
                    onChange={(e) => setCreateName(e.target.value)}
                    placeholder="Display name"
                  />
                </div>
              </div>
            </PermissionGate>
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setCreateOpen(false)}>
                {tc("action.cancel")}
              </Button>
              <PermissionGate permission="inv.manage">
                <Button
                  type="button"
                  disabled={saving || !createCode.trim() || !createName.trim()}
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
