import { AlertTriangle, Check, Pencil, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  refTableHeaderAddButtonClass,
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
import { usePermissions } from "@/hooks/use-permissions";
import {
  createInventoryTaxCategory,
  deactivateInventoryTaxCategory,
  listInventoryTaxCategories,
  updateInventoryTaxCategory,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { InventoryTaxCategory, InventoryTaxCategoryInput } from "@shared/ipc-types";

const EMPTY_DRAFT: InventoryTaxCategoryInput = {
  code: "",
  label: "",
  fr_label: "",
  en_label: "",
  description: "",
};

export function InventoryTaxCategoryManagerPanel() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can("ref.manage");
  const [taxCategories, setTaxCategories] = useState<InventoryTaxCategory[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newDraft, setNewDraft] = useState<InventoryTaxCategoryInput | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState<InventoryTaxCategoryInput>(EMPTY_DRAFT);
  const [deleteTarget, setDeleteTarget] = useState<InventoryTaxCategory | null>(null);

  const loadTaxCategories = async () => {
    setLoading(true);
    setError(null);
    try {
      const rows = await listInventoryTaxCategories();
      setTaxCategories(rows);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadTaxCategories();
  }, []);

  const beginEdit = (row: InventoryTaxCategory) => {
    if (!canManage) return;
    setEditingId(row.id);
    setEditingDraft({
      code: row.code,
      label: row.label,
      fr_label: row.fr_label ?? "",
      en_label: row.en_label ?? "",
      description: row.description ?? "",
    });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft(EMPTY_DRAFT);
  };

  const submitCreate = async () => {
    if (!canManage) return;
    if (!newDraft) return;
    setSaving(true);
    setError(null);
    try {
      await createInventoryTaxCategory({
        ...newDraft,
        fr_label: newDraft.fr_label || null,
        en_label: newDraft.en_label || null,
        description: newDraft.description || null,
      });
      setNewDraft(null);
      await loadTaxCategories();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitEdit = async (row: InventoryTaxCategory) => {
    if (!canManage) return;
    setSaving(true);
    setError(null);
    try {
      await updateInventoryTaxCategory(row.id, row.row_version, {
        ...editingDraft,
        fr_label: editingDraft.fr_label || null,
        en_label: editingDraft.en_label || null,
        description: editingDraft.description || null,
      });
      cancelEdit();
      await loadTaxCategories();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitDeactivate = async () => {
    if (!canManage) return;
    if (!deleteTarget) return;
    setSaving(true);
    setError(null);
    try {
      await deactivateInventoryTaxCategory(deleteTarget.id, deleteTarget.row_version);
      setDeleteTarget(null);
      await loadTaxCategories();
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
            Catégories TVA articles de stock
          </span>
          <Badge variant="default" className="text-[10px]">
            v1 — published
          </Badge>
        </div>
        <Button
          variant="outline"
          size="sm"
          className={refTableHeaderAddButtonClass()}
          onClick={() => {
            setNewDraft(EMPTY_DRAFT);
            setEditingId(null);
          }}
          disabled={!canManage || saving || !!newDraft}
        >
          <Plus className="h-3.5 w-3.5" />
          Add TVA type
        </Button>
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
              <th className="px-3 py-2 text-left font-medium text-text-muted">Code</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">Label</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">FR</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">EN</th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">Status</th>
              <th className="px-3 py-2 text-right align-middle font-medium text-text-muted">
                Actions
              </th>
            </tr>
          </thead>
          <tbody>
            {newDraft && canManage ? (
              <tr className="border-b border-surface-border bg-primary/5">
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="TVA_20"
                    value={newDraft.code}
                    onChange={(e) =>
                      setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), code: e.target.value }))
                    }
                    className="h-7 text-sm"
                    autoFocus
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="TVA 20%"
                    value={newDraft.label}
                    onChange={(e) =>
                      setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), label: e.target.value }))
                    }
                    className="h-7 text-sm"
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="TVA 20%"
                    value={newDraft.fr_label ?? ""}
                    onChange={(e) =>
                      setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), fr_label: e.target.value }))
                    }
                    className="h-7 text-sm"
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Input
                    placeholder="VAT 20%"
                    value={newDraft.en_label ?? ""}
                    onChange={(e) =>
                      setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), en_label: e.target.value }))
                    }
                    className="h-7 text-sm"
                  />
                </td>
                <td className="px-3 py-1.5">
                  <Badge variant="secondary" className="text-[10px]">
                    {t("editor.statusNew")}
                  </Badge>
                </td>
                <td className="px-3 py-1.5 text-right align-middle">
                  <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                    <Button
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      onClick={() => void submitCreate()}
                      disabled={saving || !newDraft.code?.trim() || !newDraft.label?.trim()}
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
                <td className="px-3 py-3 text-text-muted" colSpan={6}>
                  Loading TVA categories...
                </td>
              </tr>
            ) : taxCategories.length === 0 ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={6}>
                  No TVA categories yet.
                </td>
              </tr>
            ) : (
              taxCategories.map((row) => {
                const isEditing = editingId === row.id;
                return (
                  <tr
                    key={row.id}
                    className={`border-b border-surface-border hover:bg-surface-1 ${isEditing ? "bg-primary/5" : ""}`}
                  >
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.code}
                          onChange={(e) => setEditingDraft((s) => ({ ...s, code: e.target.value }))}
                        />
                      ) : (
                        <span className="font-mono text-xs">{row.code}</span>
                      )}
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
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.fr_label ?? ""}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, fr_label: e.target.value }))
                          }
                        />
                      ) : (
                        <span className="text-xs text-text-muted">{row.fr_label ?? "—"}</span>
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.en_label ?? ""}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, en_label: e.target.value }))
                          }
                        />
                      ) : (
                        <span className="text-xs text-text-muted">{row.en_label ?? "—"}</span>
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      <Badge
                        variant={row.is_active === 1 ? "default" : "secondary"}
                        className="text-[10px]"
                      >
                        {row.is_active === 1
                          ? t("editor.statusActive")
                          : t("editor.statusInactive")}
                      </Badge>
                    </td>
                    <td className="px-3 py-1.5 text-right align-middle">
                      {canManage && isEditing ? (
                        <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                          <Button
                            variant="ghost"
                            size="icon"
                            className={refTableIconButtonClass()}
                            onClick={() => void submitEdit(row)}
                            disabled={
                              saving || !editingDraft.code.trim() || !editingDraft.label.trim()
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
                      ) : canManage ? (
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
                          {row.is_active === 1 ? (
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
            <DialogTitle>Deactivate TVA category</DialogTitle>
            <DialogDescription>{`Deactivate TVA category ${deleteTarget?.code ?? ""}?`}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={saving}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={() => void submitDeactivate()} disabled={saving}>
              Deactivate
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
