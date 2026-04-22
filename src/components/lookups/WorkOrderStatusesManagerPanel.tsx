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
import { usePermissions } from "@/hooks/use-permissions";
import { listWorkOrderStatuses, updateWorkOrderStatus } from "@/services/wo-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderStatusOption } from "@shared/ipc-types";

export function WorkOrderStatusesManagerPanel() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can("ref.manage");

  const [rows, setRows] = useState<WorkOrderStatusOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState({ label: "", color: "" });

  const loadRows = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await listWorkOrderStatuses();
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

  const beginEdit = (row: WorkOrderStatusOption) => {
    setEditingId(row.id);
    setEditingDraft({ label: row.label, color: row.color });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft({ label: "", color: "" });
  };

  const submitEdit = async (row: WorkOrderStatusOption) => {
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderStatus(row.id, {
        label: editingDraft.label,
        color: editingDraft.color,
      });
      cancelEdit();
      await loadRows();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-col gap-1 border-b border-surface-border px-4 py-3">
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate text-sm font-semibold text-text-primary">
            {t("woStatuses.panelTitle")}
          </span>
          <Badge variant="default" className="text-[10px]">
            v1 — {t("browser.status.published")}
          </Badge>
        </div>
        <p className="text-xs text-text-muted">{t("woStatuses.lifecycleHint")}</p>
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
                {t("woStatuses.colSequence")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woStatuses.colCode")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woStatuses.colMacro")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woStatuses.colLabel")}
              </th>
              <th className="px-3 py-2 text-left align-middle font-medium text-text-muted">
                {t("woStatuses.colColor")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woStatuses.colTerminal")}
              </th>
              <th className="px-3 py-2 text-left font-medium text-text-muted">
                {t("woStatuses.colSystem")}
              </th>
              <th className="px-3 py-2 text-right align-middle font-medium text-text-muted">
                {t("woStatuses.colActions")}
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
                  {t("woStatuses.empty")}
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
                    <td className="px-3 py-1.5 font-mono text-xs">{row.sequence}</td>
                    <td className="px-3 py-1.5">
                      <div className="flex items-center gap-1">
                        <span className="inline-flex" title={t("woStatuses.lockedHint")}>
                          <Lock className="h-3 w-3 shrink-0 text-status-warning" aria-hidden />
                        </span>
                        <span className="font-mono text-xs">{row.code}</span>
                      </div>
                    </td>
                    <td className="px-3 py-1.5 font-mono text-xs text-text-muted">
                      {row.macro_state}
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
                    <td className="px-3 py-1.5 align-middle">
                      {isEditing ? (
                        <Input
                          className="h-7 font-mono text-sm"
                          value={editingDraft.color}
                          onChange={(e) =>
                            setEditingDraft((s) => ({ ...s, color: e.target.value }))
                          }
                          disabled={!canManage}
                          placeholder="#RRGGBB"
                        />
                      ) : (
                        <ReferenceColorSwatchHex color={row.color} size="md" />
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      <Badge variant="outline" className="text-[10px]">
                        {row.is_terminal ? t("woStatuses.terminalYes") : t("woStatuses.terminalNo")}
                      </Badge>
                    </td>
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
                                !/^#[0-9A-Fa-f]{6}$/.test(editingDraft.color.trim())
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
