import { Check, Lock, Pencil, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceColorSwatchHex } from "@/components/lookups/ReferenceColorSwatchHex";
import {
  ReferenceValueTable,
  ReferenceValueTableBody,
  ReferenceValueTableCell,
  ReferenceValueTableGrid,
  ReferenceValueTableHead,
  ReferenceValueTableHeadCell,
  ReferenceValueTableRow,
} from "@/components/lookups/ReferenceValueTable";
import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  REF_TABLE_BADGE_CLASS,
  REF_TABLE_EMPTY_ACTIONS_MARK,
  REF_TABLE_TITLE_CLASS,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import type { ReferenceTableCapabilities } from "@/components/lookups/reference-value-table-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { usePermissions } from "@/hooks/use-permissions";
import { listWorkOrderStatuses, updateWorkOrderStatus } from "@/services/wo-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderStatusOption } from "@shared/ipc-types";

export function WorkOrderStatusesHost() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can("ref.manage");
  const capabilities: ReferenceTableCapabilities = {
    canCreate: false,
    canUpdate: canManage,
    canDeactivateOrDelete: false,
    canToggleActive: false
  };
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
      setRows(await listWorkOrderStatuses());
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
    if (!capabilities.canUpdate || row.is_system) return;
    setEditingId(row.id);
    setEditingDraft({ label: row.label, color: row.color });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft({ label: "", color: "" });
  };

  const submitEdit = async (row: WorkOrderStatusOption) => {
    if (!capabilities.canUpdate || row.is_system) return;
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
    <ReferenceValueTable
      loading={loading && rows.length === 0}
      error={error}
      title={
        <>
          <span className={REF_TABLE_TITLE_CLASS}>{t("woStatuses.panelTitle")}</span>
          <Badge variant="default" className={REF_TABLE_BADGE_CLASS}>
            {t("synthetic.published")}
          </Badge>
        </>
      }
      banner={
        <p className="border-b border-surface-border px-4 py-2 text-xs text-text-muted">
          {t("woStatuses.lifecycleHint")}
        </p>
      }
      emptyLabel={t("woStatuses.empty")}
      showEmpty={rows.length === 0}
    >
      <ReferenceValueTableGrid>
        <ReferenceValueTableHead>
          <tr>
            <ReferenceValueTableHeadCell>{t("woStatuses.colSequence")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woStatuses.colCode")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woStatuses.colMacro")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woStatuses.colLabel")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woStatuses.colColor")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woStatuses.colTerminal")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woStatuses.colSystem")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell align="right">
              {t("woStatuses.colActions")}
            </ReferenceValueTableHeadCell>
          </tr>
        </ReferenceValueTableHead>
        <ReferenceValueTableBody>
          {rows.map((row) => {
            const isEditing = editingId === row.id;
            return (
              <ReferenceValueTableRow key={row.id} highlighted={isEditing}>
                <ReferenceValueTableCell className="font-mono text-xs">
                  {row.sequence}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <div className="flex items-center gap-1">
                    <span className="inline-flex" title={t("woStatuses.lockedHint")}>
                      <Lock className="h-3 w-3 shrink-0 text-status-warning" aria-hidden />
                    </span>
                    <span className="font-mono text-xs">{row.code}</span>
                  </div>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell className="font-mono text-xs text-text-muted">
                  {row.macro_state}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.label}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, label: e.target.value }))}
                      disabled={!capabilities.canUpdate}
                    />
                  ) : (
                    row.label
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 font-mono text-sm"
                      value={editingDraft.color}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, color: e.target.value }))}
                      disabled={!capabilities.canUpdate}
                      placeholder={t("synthetic.woStatuses.colorPlaceholder")}
                    />
                  ) : (
                    <ReferenceColorSwatchHex color={row.color} size="md" />
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Badge variant="outline" className={REF_TABLE_BADGE_CLASS}>
                    {row.is_terminal ? t("woStatuses.terminalYes") : t("woStatuses.terminalNo")}
                  </Badge>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Badge
                    variant={row.is_system ? "outline" : "secondary"}
                    className={REF_TABLE_BADGE_CLASS}
                  >
                    {row.is_system ? t("woPriorities.systemBadge") : t("woPriorities.customBadge")}
                  </Badge>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell align="right">
                  {capabilities.canUpdate ? (
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
                          disabled={saving || row.is_system}
                          aria-label={t("editor.edit")}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                      </div>
                    )
                  ) : (
                    <span className="text-xs text-text-muted">{REF_TABLE_EMPTY_ACTIONS_MARK}</span>
                  )}
                </ReferenceValueTableCell>
              </ReferenceValueTableRow>
            );
          })}
        </ReferenceValueTableBody>
      </ReferenceValueTableGrid>
    </ReferenceValueTable>
  );
}
