import { Check, Pencil, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
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
  refTableHeaderAddButtonClass,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import type { ReferenceTableCapabilities } from "@/components/lookups/reference-value-table-types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
import { P } from "@shared/rbac/permissions.generated";

const EMPTY_DRAFT: InventoryTaxCategoryInput = {
  code: "",
  label: "",
  fr_label: "",
  en_label: "",
  description: "",
};

export function InventoryTaxCategoryHost() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const capabilities: ReferenceTableCapabilities = {
    canCreate: canManage,
    canUpdate: canManage,
    canDeactivateOrDelete: canManage,
    canToggleActive: false,
  };
  const [rows, setRows] = useState<InventoryTaxCategory[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newDraft, setNewDraft] = useState<InventoryTaxCategoryInput | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState<InventoryTaxCategoryInput>(EMPTY_DRAFT);
  const [deleteTarget, setDeleteTarget] = useState<InventoryTaxCategory | null>(null);

  const loadRows = async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await listInventoryTaxCategories());
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadRows();
  }, []);

  const beginEdit = (row: InventoryTaxCategory) => {
    if (!capabilities.canUpdate) return;
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
    if (!capabilities.canCreate || !newDraft) return;
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
      await loadRows();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitEdit = async (row: InventoryTaxCategory) => {
    if (!capabilities.canUpdate) return;
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
      await loadRows();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitDeactivate = async () => {
    if (!capabilities.canDeactivateOrDelete || !deleteTarget) return;
    setSaving(true);
    setError(null);
    try {
      await deactivateInventoryTaxCategory(deleteTarget.id, deleteTarget.row_version);
      setDeleteTarget(null);
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
          <span className={REF_TABLE_TITLE_CLASS}>{t("synthetic.taxCategories.title")}</span>
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
              setNewDraft(EMPTY_DRAFT);
              setEditingId(null);
            }}
            disabled={saving || !!newDraft}
          >
            <Plus className="h-3.5 w-3.5" />
            {t("synthetic.taxCategories.add")}
          </Button>
        ) : null
      }
      emptyLabel={t("synthetic.taxCategories.empty")}
      showEmpty={rows.length === 0 && !newDraft}
      confirm={
        deleteTarget
          ? {
              open: true,
              title: t("synthetic.taxCategories.deactivateTitle"),
              description: t("synthetic.taxCategories.deactivateDescription", {
                code: deleteTarget.code,
              }),
              confirmLabel: t("editor.deactivate"),
              cancelLabel: t("editor.cancel"),
              busy: saving,
              onOpenChange: (open) => {
                if (!open) setDeleteTarget(null);
              },
              onConfirm: () => void submitDeactivate(),
            }
          : null
      }
    >
      <ReferenceValueTableGrid>
        <ReferenceValueTableHead>
          <tr>
            <ReferenceValueTableHeadCell>{t("editor.colCode")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("editor.colLabel")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("synthetic.taxCategories.colFr")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("synthetic.taxCategories.colEn")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("editor.colStatus")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell align="right">
              {t("editor.colActions")}
            </ReferenceValueTableHeadCell>
          </tr>
        </ReferenceValueTableHead>
        <ReferenceValueTableBody>
          {newDraft && capabilities.canCreate ? (
            <ReferenceValueTableRow highlighted>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("synthetic.taxCategories.codePlaceholder")}
                  value={newDraft.code}
                  onChange={(e) =>
                    setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), code: e.target.value }))
                  }
                  className="h-7 text-sm"
                  autoFocus
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("synthetic.taxCategories.labelPlaceholder")}
                  value={newDraft.label}
                  onChange={(e) =>
                    setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), label: e.target.value }))
                  }
                  className="h-7 text-sm"
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("synthetic.taxCategories.frPlaceholder")}
                  value={newDraft.fr_label ?? ""}
                  onChange={(e) =>
                    setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), fr_label: e.target.value }))
                  }
                  className="h-7 text-sm"
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("synthetic.taxCategories.enPlaceholder")}
                  value={newDraft.en_label ?? ""}
                  onChange={(e) =>
                    setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), en_label: e.target.value }))
                  }
                  className="h-7 text-sm"
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Badge variant="secondary" className={REF_TABLE_BADGE_CLASS}>
                  {t("editor.statusNew")}
                </Badge>
              </ReferenceValueTableCell>
              <ReferenceValueTableCell align="right">
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
              </ReferenceValueTableCell>
            </ReferenceValueTableRow>
          ) : null}
          {rows.map((row) => {
            const isEditing = editingId === row.id;
            return (
              <ReferenceValueTableRow key={row.id} highlighted={isEditing}>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.code}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, code: e.target.value }))}
                    />
                  ) : (
                    <span className="font-mono text-xs">{row.code}</span>
                  )}
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
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.fr_label ?? ""}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, fr_label: e.target.value }))}
                    />
                  ) : (
                    <span className="text-xs text-text-muted">{row.fr_label ?? "—"}</span>
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.en_label ?? ""}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, en_label: e.target.value }))}
                    />
                  ) : (
                    <span className="text-xs text-text-muted">{row.en_label ?? "—"}</span>
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Badge
                    variant={row.is_active === 1 ? "default" : "secondary"}
                    className={REF_TABLE_BADGE_CLASS}
                  >
                    {row.is_active === 1 ? t("editor.statusActive") : t("editor.statusInactive")}
                  </Badge>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell align="right">
                  {capabilities.canUpdate && isEditing ? (
                    <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                      <Button
                        variant="ghost"
                        size="icon"
                        className={refTableIconButtonClass()}
                        onClick={() => void submitEdit(row)}
                        disabled={saving || !editingDraft.code.trim() || !editingDraft.label.trim()}
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
                  ) : capabilities.canUpdate ? (
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
                      {capabilities.canDeactivateOrDelete && row.is_active === 1 ? (
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
