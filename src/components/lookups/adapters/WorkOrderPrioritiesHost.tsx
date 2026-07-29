import { Check, Lock, Pencil, Trash2, X } from "lucide-react";
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
  refTableDisabledActionTitle,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import type { ReferenceTableCapabilities } from "@/components/lookups/reference-value-table-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import { listWorkOrderPriorities, updateWorkOrderPriority } from "@/services/wo-service";
import { refreshWorkOrderPrioritiesCatalog } from "@/stores/work-order-priorities-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrderPriorityOption } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function WorkOrderPrioritiesHost() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const capabilities: ReferenceTableCapabilities = {
    canCreate: false,
    canUpdate: canManage,
    canDeactivateOrDelete: canManage,
    canToggleActive: canManage,
  };
  const [rows, setRows] = useState<WorkOrderPriorityOption[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState({ label: "", label_fr: "" });
  const [deactivateTarget, setDeactivateTarget] = useState<WorkOrderPriorityOption | null>(null);

  const loadRows = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await listWorkOrderPriorities());
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadRows();
  }, [loadRows]);

  const afterMutation = async () => {
    await loadRows();
    await refreshWorkOrderPrioritiesCatalog();
  };

  const beginEdit = (row: WorkOrderPriorityOption) => {
    if (!capabilities.canUpdate || row.is_system) return;
    setEditingId(row.id);
    setEditingDraft({ label: row.label, label_fr: row.label_fr });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft({ label: "", label_fr: "" });
  };

  const submitEdit = async (row: WorkOrderPriorityOption) => {
    if (!capabilities.canUpdate || row.is_system) return;
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderPriority(row.id, editingDraft);
      cancelEdit();
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const toggleActive = async (row: WorkOrderPriorityOption, next: boolean) => {
    if (!capabilities.canToggleActive) return;
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

  const submitDeactivate = async () => {
    if (!capabilities.canDeactivateOrDelete || !deactivateTarget) return;
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderPriority(deactivateTarget.id, { is_active: false });
      setDeactivateTarget(null);
      await afterMutation();
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
          <span className={REF_TABLE_TITLE_CLASS}>{t("woPriorities.panelTitle")}</span>
          <Badge variant="default" className={REF_TABLE_BADGE_CLASS}>
            {t("synthetic.published")}
          </Badge>
        </>
      }
      emptyLabel={t("woPriorities.empty")}
      showEmpty={rows.length === 0}
      confirm={
        deactivateTarget
          ? {
              open: true,
              title: t("woPriorities.deactivateTitle"),
              description: t("woPriorities.deactivateDescription", {
                code: deactivateTarget.code,
              }),
              confirmLabel: t("editor.deactivate"),
              cancelLabel: t("editor.cancel"),
              busy: saving,
              onOpenChange: (open) => {
                if (!open) setDeactivateTarget(null);
              },
              onConfirm: () => void submitDeactivate(),
            }
          : null
      }
    >
      <ReferenceValueTableGrid>
        <ReferenceValueTableHead>
          <tr>
            <ReferenceValueTableHeadCell>{t("woPriorities.colLevel")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woPriorities.colCode")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woPriorities.colSystem")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("woPriorities.colLabelEn")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("woPriorities.colLabelFr")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woPriorities.colColor")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woPriorities.colActive")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell align="right">
              {t("woPriorities.colActions")}
            </ReferenceValueTableHeadCell>
          </tr>
        </ReferenceValueTableHead>
        <ReferenceValueTableBody>
          {rows.map((row) => {
            const isEditing = editingId === row.id;
            const switchDisabled = saving || !capabilities.canToggleActive;
            const switchTitle = switchDisabled
              ? refTableDisabledActionTitle({
                  missingPermission: !capabilities.canToggleActive,
                  t,
                })
              : undefined;
            const trashDisabled =
              saving || row.is_system || !row.is_active || !capabilities.canDeactivateOrDelete;
            const trashTitle = trashDisabled
              ? refTableDisabledActionTitle({
                  isSystem: row.is_system,
                  isInactive: !row.is_active,
                  missingPermission: !capabilities.canDeactivateOrDelete,
                  t,
                })
              : undefined;

            return (
              <ReferenceValueTableRow key={row.id} highlighted={isEditing}>
                <ReferenceValueTableCell>
                  <div className="flex items-center gap-1">
                    {row.is_system ? (
                      <span title={t("woPriorities.lockedHint")}>
                        <Lock className="h-3 w-3 shrink-0 text-status-warning" aria-hidden />
                      </span>
                    ) : null}
                    <span className="font-mono text-xs">{row.level}</span>
                  </div>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell className="font-mono text-xs">
                  {row.code}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Badge
                    variant={row.is_system ? "outline" : "secondary"}
                    className={REF_TABLE_BADGE_CLASS}
                  >
                    {row.is_system ? t("woPriorities.systemBadge") : t("woPriorities.customBadge")}
                  </Badge>
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
                      className="h-7 text-sm"
                      value={editingDraft.label_fr}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, label_fr: e.target.value }))}
                      disabled={!capabilities.canUpdate}
                    />
                  ) : (
                    row.label_fr
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <ReferenceColorSwatchHex color={row.hex_color} />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <div className="flex items-center gap-2">
                    <span title={switchTitle}>
                      <Switch
                        checked={row.is_active}
                        onCheckedChange={(value) => void toggleActive(row, value)}
                        disabled={switchDisabled}
                        aria-label={t("woPriorities.colActive")}
                      />
                    </span>
                    <span className="text-sm text-text-muted">
                      {row.is_active ? t("woPriorities.active") : t("woPriorities.inactive")}
                    </span>
                  </div>
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
                            saving || !editingDraft.label.trim() || !editingDraft.label_fr.trim()
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
                          title={
                            row.is_system
                              ? refTableDisabledActionTitle({ isSystem: true, t })
                              : undefined
                          }
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          className={refTableIconButtonClass()}
                          onClick={() => setDeactivateTarget(row)}
                          disabled={trashDisabled}
                          aria-label={t("editor.deactivate")}
                          title={trashTitle}
                        >
                          <Trash2 className="h-3.5 w-3.5" />
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
