import { AlertTriangle, Check, Lock, Pencil, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceColorSwatchHex } from "@/components/lookups/ReferenceColorSwatchHex";
import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import { listWorkOrderPriorities, updateWorkOrderPriority } from "@/services/wo-service";
import { refreshWorkOrderPrioritiesCatalog } from "@/stores/work-order-priorities-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderPriorityOption } from "@shared/ipc-types";

export function WorkOrderPrioritiesManagerPanel() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can("ref.manage");

  const [rows, setRows] = useState<WorkOrderPriorityOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState({ label: "", label_fr: "" });

  const loadRows = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await listWorkOrderPriorities();
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

  const beginEdit = (row: WorkOrderPriorityOption) => {
    setEditingId(row.id);
    setEditingDraft({ label: row.label, label_fr: row.label_fr });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft({ label: "", label_fr: "" });
  };

  const afterMutation = async () => {
    await loadRows();
    await refreshWorkOrderPrioritiesCatalog();
  };

  const submitEdit = async (row: WorkOrderPriorityOption) => {
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderPriority(row.id, {
        label: editingDraft.label,
        label_fr: editingDraft.label_fr,
      });
      cancelEdit();
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const toggleActive = async (row: WorkOrderPriorityOption, next: boolean) => {
    if (!canManage) return;
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderPriority(row.id, { is_active: next });
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
            {t("woPriorities.panelTitle")}
          </span>
          <Badge variant="default" className="text-[10px]">
            v1 — {t("browser.status.published")}
          </Badge>
        </div>
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
                {t("woPriorities.colLevel")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woPriorities.colCode")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woPriorities.colSystem")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woPriorities.colLabelEn")}
              </th>
              <th className="px-3 py-2 text-left align-middle font-medium text-text-muted">
                {t("woPriorities.colLabelFr")}
              </th>
              <th className="px-3 py-2 text-left align-middle font-medium text-text-muted">
                {t("woPriorities.colColor")}
              </th>
              <th className="px-3 py-2 text-left align-middle font-medium text-text-muted">
                {t("woPriorities.colActive")}
              </th>
              <th className="px-3 py-2 text-right align-middle font-medium text-text-muted">
                {t("woPriorities.colActions")}
              </th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={8}>
                  {t("browser.loadingSets")}
                </td>
              </tr>
            ) : rows.length === 0 ? (
              <tr>
                <td className="px-3 py-3 text-text-muted" colSpan={8}>
                  {t("woPriorities.empty")}
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
                      <div className="flex items-center gap-1">
                        {row.is_system ? (
                          <span title={t("woPriorities.lockedHint")}>
                            <Lock className="h-3 w-3 shrink-0 text-status-warning" aria-hidden />
                          </span>
                        ) : null}
                        <span className="font-mono text-xs">{row.level}</span>
                      </div>
                    </td>
                    <td className="px-3 py-1.5 font-mono text-xs">{row.code}</td>
                    <td className="px-3 py-1.5">
                      <Badge
                        variant={row.is_system ? "outline" : "secondary"}
                        className="text-[10px]"
                      >
                        {row.is_system
                          ? t("woPriorities.systemBadge")
                          : t("woPriorities.customBadge")}
                      </Badge>
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.label}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, label: e.target.value }))
                          }
                          disabled={!canManage}
                        />
                      ) : (
                        row.label
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          className="h-7 text-sm"
                          value={editingDraft.label_fr}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, label_fr: e.target.value }))
                          }
                          disabled={!canManage}
                        />
                      ) : (
                        row.label_fr
                      )}
                    </td>
                    <td className="px-3 py-1.5 align-middle">
                      <ReferenceColorSwatchHex color={row.hex_color} />
                    </td>
                    <td className="px-3 py-1.5 align-middle">
                      <div className="flex items-center gap-2">
                        <Switch
                          checked={row.is_active}
                          onCheckedChange={(v) => void toggleActive(row, v)}
                          disabled={saving || !canManage}
                          aria-label={t("woPriorities.colActive")}
                        />
                        <span className="text-sm text-text-muted">
                          {row.is_active ? t("woPriorities.active") : t("woPriorities.inactive")}
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
                                !editingDraft.label_fr.trim()
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
    </div>
  );
}
