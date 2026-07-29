import { Check, Lock, Pencil, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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
  refTableHeaderAddButtonClass,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import type { ReferenceTableCapabilities } from "@/components/lookups/reference-value-table-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { P } from "@shared/rbac/permissions.generated";

export function WorkOrderTypesHost() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const capabilities: ReferenceTableCapabilities = {
    canCreate: canManage,
    canUpdate: canManage,
    canDeactivateOrDelete: canManage,
    canToggleActive: canManage,
  };
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
      setRows(await listWorkOrderTypes());
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
    await refreshWorkOrderTypesCatalog();
  };

  const beginEdit = (row: WorkOrderTypeOption) => {
    if (!capabilities.canUpdate || row.is_system) return;
    setEditingId(row.id);
    setEditingDraft({ code: row.code, label: row.label });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft({ code: "", label: "" });
  };

  const submitCreate = async () => {
    if (!capabilities.canCreate || !newDraft) return;
    setSaving(true);
    setError(null);
    try {
      await createWorkOrderType(newDraft);
      setNewDraft(null);
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitEdit = async (row: WorkOrderTypeOption) => {
    if (!capabilities.canUpdate || row.is_system) return;
    setSaving(true);
    setError(null);
    try {
      await updateWorkOrderType(row.id, {
        code: editingDraft.code,
        label: editingDraft.label,
      });
      cancelEdit();
      await afterMutation();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const toggleActive = async (row: WorkOrderTypeOption, next: boolean) => {
    if (!capabilities.canToggleActive) return;
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
    if (!capabilities.canDeactivateOrDelete || !deleteTarget || deleteTarget.is_system) return;
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
    <ReferenceValueTable
      loading={loading && rows.length === 0}
      error={error}
      title={
        <>
          <span className={REF_TABLE_TITLE_CLASS}>{t("woTypes.panelTitle")}</span>
          <Badge variant="default" className={REF_TABLE_BADGE_CLASS}>
            {t("synthetic.published")}
          </Badge>
        </>
      }
      toolbar={
        capabilities.canCreate ? (
          <Button
            variant="outline"
            size="sm"
            className={refTableHeaderAddButtonClass()}
            onClick={() => {
              setNewDraft({ code: "", label: "" });
              setEditingId(null);
            }}
            disabled={saving || !!newDraft}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("woTypes.addType")}
          </Button>
        ) : null
      }
      emptyLabel={t("woTypes.empty")}
      showEmpty={rows.length === 0 && !newDraft}
      confirm={
        deleteTarget
          ? {
              open: true,
              title: t("woTypes.deleteTitle"),
              description: t("woTypes.deleteDescription", { code: deleteTarget.code }),
              confirmLabel: t("alias.delete"),
              cancelLabel: t("editor.cancel"),
              busy: saving,
              onOpenChange: (open) => {
                if (!open) setDeleteTarget(null);
              },
              onConfirm: () => void submitDelete(),
            }
          : null
      }
    >
      <ReferenceValueTableGrid>
        <ReferenceValueTableHead>
          <tr>
            <ReferenceValueTableHeadCell>{t("woTypes.colCode")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woTypes.colLabel")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woTypes.colSystem")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("woTypes.colActive")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell align="right">
              {t("woTypes.colActions")}
            </ReferenceValueTableHeadCell>
          </tr>
        </ReferenceValueTableHead>
        <ReferenceValueTableBody>
          {newDraft && capabilities.canCreate ? (
            <ReferenceValueTableRow highlighted>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("synthetic.woTypes.codePlaceholder")}
                  value={newDraft.code}
                  onChange={(e) => setNewDraft((s) => (s ? { ...s, code: e.target.value } : s))}
                  className="h-7 font-mono text-xs"
                  autoFocus
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("editor.labelPlaceholder")}
                  value={newDraft.label}
                  onChange={(e) => setNewDraft((s) => (s ? { ...s, label: e.target.value } : s))}
                  className="h-7 text-sm"
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Badge variant="secondary" className={REF_TABLE_BADGE_CLASS}>
                  {t("woTypes.customBadge")}
                </Badge>
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <span className="text-xs text-text-muted">{t("woTypes.active")}</span>
              </ReferenceValueTableCell>
              <ReferenceValueTableCell align="right">
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
              </ReferenceValueTableCell>
            </ReferenceValueTableRow>
          ) : null}
          {rows.map((row) => {
            const isEditing = editingId === row.id;
            const switchDisabled = saving || !capabilities.canToggleActive;
            const switchTitle = switchDisabled
              ? refTableDisabledActionTitle({
                  missingPermission: !capabilities.canToggleActive,
                  t,
                })
              : undefined;
            const trashDisabled = saving || row.is_system || !capabilities.canDeactivateOrDelete;
            const trashTitle = trashDisabled
              ? refTableDisabledActionTitle({
                  isSystem: row.is_system,
                  missingPermission: !capabilities.canDeactivateOrDelete,
                  t,
                })
              : undefined;
            return (
              <ReferenceValueTableRow key={row.id} highlighted={isEditing}>
                <ReferenceValueTableCell>
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
                        onChange={(e) => setEditingDraft((s) => ({ ...s, code: e.target.value }))}
                        disabled={row.is_system}
                      />
                    ) : (
                      <span className="font-mono text-xs">{row.code}</span>
                    )}
                  </div>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.label}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, label: e.target.value }))}
                    />
                  ) : (
                    row.label
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Badge
                    variant={row.is_system ? "outline" : "secondary"}
                    className={REF_TABLE_BADGE_CLASS}
                  >
                    {row.is_system ? t("woTypes.systemBadge") : t("woTypes.customBadge")}
                  </Badge>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <div className="flex items-center gap-2">
                    <span title={switchTitle}>
                      <Switch
                        checked={row.is_active}
                        onCheckedChange={(value) => void toggleActive(row, value)}
                        disabled={switchDisabled}
                        aria-label={t("woTypes.colActive")}
                      />
                    </span>
                    <span className="text-sm text-text-muted">
                      {row.is_active ? t("woTypes.active") : t("woTypes.inactive")}
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
                            saving || !editingDraft.label.trim() || !editingDraft.code.trim()
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
                          onClick={() => setDeleteTarget(row)}
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
