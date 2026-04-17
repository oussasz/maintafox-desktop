import type { ColumnDef } from "@tanstack/react-table";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useState } from "react";

import { PermissionGate } from "@/components/PermissionGate";
import { DataTable } from "@/components/data/DataTable";
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
import {
  createInventoryStockLocation,
  createInventoryWarehouse,
  listInventoryLocations,
  updateInventoryStockLocation,
  updateInventoryWarehouse,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import { useInventoryStore } from "@/stores/inventory-store";
import type { StockLocation, Warehouse } from "@shared/ipc-types";

export type WarehouseLocationPanelHandle = {
  openCreateWarehouse: () => void;
};

type Props = {
  searchFilter: string;
};

export const WarehouseLocationPanel = forwardRef<WarehouseLocationPanelHandle, Props>(
  function WarehouseLocationPanel({ searchFilter }, ref) {
    const warehouses = useInventoryStore((s) => s.warehouses);
    const loadAll = useInventoryStore((s) => s.loadAll);
    const saving = useInventoryStore((s) => s.saving);

    const [dialogOpen, setDialogOpen] = useState(false);
    const [editingWarehouse, setEditingWarehouse] = useState<Warehouse | null>(null);
    const [pendingCreate, setPendingCreate] = useState(false);

    const [whCode, setWhCode] = useState("");
    const [whName, setWhName] = useState("");
    const [whActive, setWhActive] = useState(true);

    const [dialogLocations, setDialogLocations] = useState<StockLocation[]>([]);
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

    const reloadDialogLocations = useCallback(async (warehouseId: number) => {
      const rows = await listInventoryLocations(warehouseId);
      setDialogLocations(rows);
    }, []);

    useEffect(() => {
      if (!editingWarehouse?.id) {
        setDialogLocations([]);
        return;
      }
      void reloadDialogLocations(editingWarehouse.id).catch(() => setDialogLocations([]));
    }, [editingWarehouse?.id, reloadDialogLocations]);

    const resetWarehouseForm = () => {
      setWhCode("");
      setWhName("");
      setWhActive(true);
      setNewLocCode("");
      setNewLocName("");
      setNewLocDefault(false);
      setEditLoc(null);
      setLocError(null);
    };

    useImperativeHandle(ref, () => ({
      openCreateWarehouse: () => {
        setEditingWarehouse(null);
        setPendingCreate(true);
        resetWarehouseForm();
        setDialogOpen(true);
      },
    }));

    const filteredWarehouses = useMemo(() => {
      const q = searchFilter.trim().toLowerCase();
      if (!q) return warehouses;
      return warehouses.filter(
        (w) => w.code.toLowerCase().includes(q) || w.name.toLowerCase().includes(q),
      );
    }, [warehouses, searchFilter]);

    const openEdit = (w: Warehouse) => {
      setPendingCreate(false);
      setEditingWarehouse(w);
      setWhCode(w.code);
      setWhName(w.name);
      setWhActive(w.is_active === 1);
      setNewLocCode("");
      setNewLocName("");
      setNewLocDefault(false);
      setEditLoc(null);
      setLocError(null);
      setDialogOpen(true);
    };

    const saveWarehouseCore = async () => {
      if (pendingCreate || !editingWarehouse) {
        const created = await createInventoryWarehouse({
          code: whCode.trim(),
          name: whName.trim(),
        });
        setPendingCreate(false);
        setEditingWarehouse(created);
        setWhCode(created.code);
        setWhName(created.name);
        setWhActive(created.is_active === 1);
        await loadAll();
        await reloadDialogLocations(created.id);
        return;
      }
      await updateInventoryWarehouse(editingWarehouse.id, {
        name: whName.trim(),
        is_active: whActive,
      });
      await loadAll();
      const refreshed = useInventoryStore.getState().warehouses.find((x) => x.id === editingWarehouse.id);
      if (refreshed) setEditingWarehouse(refreshed);
    };

    const handleSaveWarehouse = async () => {
      if (!whCode.trim() || !whName.trim()) return;
      try {
        setLocError(null);
        await saveWarehouseCore();
      } catch (err) {
        setLocError(toErrorMessage(err));
      }
    };

    const handleAddLocation = async () => {
      if (!newLocCode.trim() || !newLocName.trim()) return;
      setLocSaving(true);
      setLocError(null);
      try {
        let warehouseId = editingWarehouse?.id ?? null;
        if (!warehouseId) {
          if (!whCode.trim() || !whName.trim()) {
            setLocError("Save warehouse code and name first.");
            return;
          }
          const created = await createInventoryWarehouse({
            code: whCode.trim(),
            name: whName.trim(),
          });
          warehouseId = created.id;
          setPendingCreate(false);
          setEditingWarehouse(created);
          setWhCode(created.code);
          setWhName(created.name);
          setWhActive(created.is_active === 1);
        }

        await createInventoryStockLocation({
          warehouse_id: warehouseId,
          code: newLocCode.trim(),
          name: newLocName.trim(),
          is_default: newLocDefault,
        });
        setNewLocCode("");
        setNewLocName("");
        setNewLocDefault(false);
        await reloadDialogLocations(warehouseId);
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
      if (!editLoc || !editLocCode.trim() || !editLocName.trim()) return;
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
        if (editingWarehouse?.id) await reloadDialogLocations(editingWarehouse.id);
        await loadAll();
      } catch (err) {
        setLocError(toErrorMessage(err));
      } finally {
        setLocSaving(false);
      }
    };

    const whColumns: ColumnDef<Warehouse>[] = useMemo(
      () => [
        { accessorKey: "code", header: "Code" },
        { accessorKey: "name", header: "Name" },
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
        { accessorKey: "created_at", header: "Created" },
      ],
      [],
    );

    const locColumns: ColumnDef<StockLocation>[] = useMemo(
      () => [
        { accessorKey: "code", header: "Code" },
        { accessorKey: "name", header: "Name" },
        {
          accessorKey: "is_default",
          header: "Default",
          cell: ({ row }) => (row.original.is_default === 1 ? "Yes" : "No"),
        },
        {
          accessorKey: "is_active",
          header: "Active",
          cell: ({ row }) => (row.original.is_active === 1 ? "Yes" : "No"),
        },
        {
          id: "actions",
          header: "",
          cell: ({ row }) => (
            <Button type="button" variant="outline" size="sm" onClick={() => beginEditLocation(row.original)}>
              Edit
            </Button>
          ),
        },
      ],
      [],
    );

    const dialogTitle = pendingCreate
      ? "New warehouse"
      : editingWarehouse
        ? `${editingWarehouse.code} — ${editingWarehouse.name}`
        : "Warehouse";

    return (
      <div className="space-y-4">
        {locError ? (
          <div className="rounded-md border border-destructive/30 bg-destructive/10 p-2 text-sm">{locError}</div>
        ) : null}

        <DataTable
          columns={whColumns}
          data={filteredWarehouses}
          isLoading={false}
          searchable={false}
          onRowClick={openEdit}
        />

        <Dialog
          open={dialogOpen}
          onOpenChange={(o) => {
            setDialogOpen(o);
            if (!o) {
              setEditingWarehouse(null);
              setPendingCreate(false);
              resetWarehouseForm();
            }
          }}
        >
          <DialogContent
            className="max-h-[90vh] max-w-2xl overflow-y-auto"
            onPointerDownOutside={(e) => e.preventDefault()}
          >
            <DialogHeader>
              <DialogTitle>{dialogTitle}</DialogTitle>
            </DialogHeader>

            <div className="space-y-4">
              <div className="grid gap-3 md:grid-cols-2">
                <div className="space-y-1">
                  <Label htmlFor="wh-code">Code</Label>
                  <Input
                    id="wh-code"
                    value={whCode}
                    onChange={(e) => setWhCode(e.target.value)}
                    disabled={!pendingCreate && !!editingWarehouse}
                    placeholder="e.g. WH01"
                  />
                </div>
                <div className="space-y-1">
                  <Label htmlFor="wh-name">Name</Label>
                  <Input
                    id="wh-name"
                    value={whName}
                    onChange={(e) => setWhName(e.target.value)}
                    placeholder="Display name"
                  />
                </div>
                {!pendingCreate && editingWarehouse ? (
                  <div className="flex items-center gap-2 md:col-span-2">
                    <Switch id="wh-active" checked={whActive} onCheckedChange={setWhActive} />
                    <Label htmlFor="wh-active">Warehouse active</Label>
                  </div>
                ) : null}
              </div>

              <PermissionGate permission="inv.manage">
                <div className="flex flex-wrap gap-2">
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => void handleSaveWarehouse()}
                    disabled={saving || !whCode.trim() || !whName.trim()}
                  >
                    {pendingCreate ? "Create warehouse" : "Save warehouse"}
                  </Button>
                </div>
              </PermissionGate>

              {pendingCreate || editingWarehouse ? (
                <div className="space-y-3 rounded-md border p-3">
                  <h4 className="text-sm font-semibold">Locations</h4>
                  {pendingCreate ? (
                    <p className="text-xs text-text-muted">
                      Adding the first location will save this warehouse and attach the location in one action.
                    </p>
                  ) : null}
                  <div className="grid gap-2 md:grid-cols-3">
                    <div className="space-y-1">
                      <Label htmlFor="new-loc-code">Location code</Label>
                      <Input
                        id="new-loc-code"
                        value={newLocCode}
                        onChange={(e) => setNewLocCode(e.target.value)}
                        placeholder="BIN-A1"
                      />
                    </div>
                    <div className="space-y-1">
                      <Label htmlFor="new-loc-name">Location name</Label>
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
                        <Label htmlFor="new-loc-def">Default bin</Label>
                      </div>
                    </div>
                  </div>
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    disabled={
                      locSaving ||
                      !newLocCode.trim() ||
                      !newLocName.trim() ||
                      (pendingCreate && (!whCode.trim() || !whName.trim()))
                    }
                    onClick={() => void handleAddLocation()}
                  >
                    {pendingCreate ? "Save warehouse & add location" : "Add location"}
                  </Button>

                  <DataTable
                    columns={locColumns}
                    data={dialogLocations}
                    searchable={false}
                    isLoading={false}
                  />

                  {editLoc ? (
                    <div className="rounded-md border bg-muted/30 p-3 space-y-2">
                      <div className="text-sm font-medium">Edit location {editLoc.code}</div>
                      <div className="grid gap-2 md:grid-cols-2">
                        <div className="space-y-1">
                          <Label htmlFor="el-code">Code</Label>
                          <Input
                            id="el-code"
                            value={editLocCode}
                            onChange={(e) => setEditLocCode(e.target.value)}
                          />
                        </div>
                        <div className="space-y-1">
                          <Label htmlFor="el-name">Name</Label>
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
                          <Label htmlFor="el-def">Default bin</Label>
                        </div>
                        <div className="flex items-center gap-2">
                          <Switch
                            id="el-act"
                            checked={editLocActive}
                            onCheckedChange={setEditLocActive}
                          />
                          <Label htmlFor="el-act">Active</Label>
                        </div>
                      </div>
                      <div className="flex gap-2">
                        <Button
                          type="button"
                          size="sm"
                          disabled={locSaving}
                          onClick={() => void saveEditLocation()}
                        >
                          Save location
                        </Button>
                        <Button type="button" size="sm" variant="outline" onClick={cancelEditLocation}>
                          Cancel
                        </Button>
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>

            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setDialogOpen(false)}>
                Close
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    );
  },
);
