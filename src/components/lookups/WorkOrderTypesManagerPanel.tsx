import { AlertTriangle, Check, Lock, Pencil, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
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
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import {
  createWorkOrderType,
  deleteWorkOrderType,
  listWorkOrderTypes,
  updateWorkOrderType,
} from "@/services/wo-service";
import { refreshWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderTypeOption } from "@shared/ipc-types";

export function WorkOrderTypesManagerPanel() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can("ref.manage");

  const [rows, setRows] = useState<WorkOrderTypeOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newDraft, setNewDraft] = useState<{ code: string; label: string } | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState({ code: "", label: "" });
  const [deleteTarget, setDeleteTarget] = useState<WorkOrderTypeOption | null>(null);

  const loadRows = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await listWorkOrderTypes();
      setRows(list);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadRows();
  }, [loadRows]);

  const beginEdit = (row: WorkOrderTypeOption) => {
    setEditingId(row.id);
    setEditingDraft({ code: row.code, label: row.label });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft({ code: "", label: "" });
  };

  const afterMutation = async () => {
    await loadRows();
    await refreshWorkOrderTypesCatalog();
  };

  const submitCreate = async () => {
    if (!newDraft) return;
    setSaving(true);
    setError(null);
    try {
      await createWorkOrderType({
        code: newDraft.code,
        label: newDraft.label,
      });
      setNewDraft(null);
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitEdit = async (row: WorkOrderTypeOption) => {
    setSaving(true);
    setError(null);
    try {
      const payload = row.is_system
        ? { label: editingDraft.label }
        : {
            code: editingDraft.code,
            label: editingDraft.label,
          };
      await updateWorkOrderType(row.id, payload);
      cancelEdit();
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const toggleActive = async (row: WorkOrderTypeOption, next: boolean) => {
    if (!canManage) return;
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderType(row.id, { is_active: next });
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitDelete = async () => {
    if (!deleteTarget) return;
    setSaving(true);
    setError(null);
    try {
      await deleteWorkOrderType(deleteTarget.id);
      setDeleteTarget(null);
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-surface-border px-4 py-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-semibold text-text-primary">
            {t("woTypes.panelTitle")}
          </span>
          <Badge variant="default" className="text-[10px]">
            v1 — {t("browser.status.published")}
          </Badge>
        </div>
        {canManage ? (
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5"
            onClick={() => {
              setNewDraft({ code: "", label: "" });
              setEditingId(null);
            }}
            disabled={saving || !!newDraft}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("woTypes.addType")}
          </Button>
        ) : null}
      </div>

      {error ? (
        <div className="flex items-center gap-2 bg-red-50 px-4 py-2 text-sm text-status-danger dark:bg-red-950/20">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          {error}
        </div>
      ) : null}

      <div className="flex-1 overflow-auto">
        <table className="w-full text-sm">
          <thead className="sticky top-0 z-10 border-b border-surface-border bg-surface-0">
            <tr>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woTypes.colCode")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woTypes.colLabel")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woTypes.colSystem")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woTypes.colActive")}
              </th>
              <th className="px-3 py-2 text-right align-middle font-medium text-text-muted">
                {t("woTypes.colActions")}
              </th>
            </tr>
          </thead>
          <tbody>
            {newDraft && canManage ? (
              <tr className="border-b border-surface-border bg-primary/5">
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="custom_type"
                    value={newDraft.code}
                    onChange={(e) => setNewDraft((s) => (s ? { ...s, code: e.target.value } : s))}
                    className="h-7 font-mono text-xs"
                    autoFocus
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Input
                    placeholder={t("editor.labelPlaceholder")}
                    value={newDraft.label}
                    onChange={(e) => setNewDraft((s) => (s ? { ...s, label: e.target.value } : s))}
                    className="h-7 text-sm"
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Badge variant="secondary" className="text-[10px]">
                    {t("woTypes.customBadge")}
                  </Badge>
                </td>
                <td className="px-3 py-1.5">
                  <span className="text-xs text-text-muted">{t("woTypes.active")}</span>
                </td>
                <td className="px-3 py-1.5 text-right align-middle">
                  <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                    <Button
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      onClick={() => void submitCreate()}
                      disabled={saving || !newDraft.code.trim() || !newDraft.label.trim()}
                    >
                      <Check className="h-3.5 w-3.5 text-status-success" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      onClick={() => setNewDraft(null)}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </td>
              </tr>
            ) : null}

            {loading ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={5}>
                  {t("browser.loadingSets")}
                </td>
              </tr>
            ) : rows.length === 0 ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={5}>
                  {t("woTypes.empty")}
                </td>
              </tr>
            ) : (
              rows.map((row) => {
                const isEditing = editingId === row.id;
                return (
                  <tr
                    key={row.id}
                    className={`border-b border-surface-border hover:bg-surface-1 ${isEditing ? "bg-primary/5" : ""}`}
                  >
                    <td className="px-3 py-1.5">
                      <div className="flex items-center gap-1.5">
                        {row.is_system ? (
                          <Lock
                            className="h-3.5 w-3.5 shrink-0 text-status-warning"
                            aria-label={t("woTypes.lockedHint")}
                          />
                        ) : null}
                        {isEditing ? (
                          <Input
                            className="h-7 font-mono text-xs"
                            value={editingDraft.code}
                            onChange={(e) =>
                              setEditingDraft((s) => ({ ...s, code: e.target.value }))
                            }
                            disabled={row.is_system}
                          />
                        ) : (
                          <span className="font-mono text-xs">{row.code}</span>
                        )}
                      </div>
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.label}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, label: e.target.value }))
                          }
                        />
                      ) : (
                        row.label
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      <Badge
                        variant={row.is_system ? "outline" : "secondary"}
                        className="text-[10px]"
                      >
                        {row.is_system ? t("woTypes.systemBadge") : t("woTypes.customBadge")}
                      </Badge>
                    </td>
                    <td className="px-3 py-1.5">
                      <div className="flex items-center gap-2">
                        <Switch
                          checked={row.is_active}
                          onCheckedChange={(v) => void toggleActive(row, v)}
                          disabled={saving || !canManage}
                          aria-label={t("woTypes.colActive")}
                        />
                        <span className="text-sm text-text-muted">
                          {row.is_active ? t("woTypes.active") : t("woTypes.inactive")}
                        </span>
                      </div>
                    </td>
                    <td className="px-3 py-1.5 text-right align-middle">
                      {canManage ? (
                        isEditing ? (
                          <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                            <Button
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              onClick={() => void submitEdit(row)}
                              disabled={
                                saving ||
                                !editingDraft.label.trim() ||
                                (!row.is_system && !editingDraft.code.trim())
                              }
                            >
                              <Check className="h-3.5 w-3.5 text-status-success" />
                            </Button>
                            <Button
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              onClick={cancelEdit}
                            >
                              <X className="h-3.5 w-3.5" />
                            </Button>
                          </div>
                        ) : (
                          <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                            <Button
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              onClick={() => beginEdit(row)}
                              disabled={saving}
                              aria-label={t("editor.edit")}
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                            {!row.is_system ? (
                              <Button
                                variant="ghost"
                                size="icon"
                                className={refTableIconButtonClass()}
                                onClick={() => setDeleteTarget(row)}
                                disabled={saving}
                                aria-label={t("editor.deactivate")}
                              >
                                <Trash2 className="h-3.5 w-3.5" />
                              </Button>
                            ) : null}
                          </div>
                        )
                      ) : (
                        <span className="text-xs text-text-muted">—</span>
                      )}
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>

      <Dialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("woTypes.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("woTypes.deleteDescription", { code: deleteTarget?.code ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={saving}>
              {t("editor.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void submitDelete()} disabled={saving}>
              {t("alias.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
