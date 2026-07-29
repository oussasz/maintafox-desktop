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
  createInventoryArticleFamily,
  deactivateInventoryArticleFamily,
  listInventoryArticleFamilies,
  updateInventoryArticleFamily,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { ArticleFamily } from "@shared/ipc-types";

interface FamilyDraft {
  code: string;
  name: string;
  description: string;
}

const EMPTY_DRAFT: FamilyDraft = { code: "", name: "", description: "" };

export function InventoryArticleFamilyHost() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can("ref.manage");
  const capabilities: ReferenceTableCapabilities = {
    canCreate: canManage,
    canUpdate: canManage,
    canDeactivateOrDelete: canManage,
    canToggleActive: false
  };
  const [families, setFamilies] = useState<ArticleFamily[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newDraft, setNewDraft] = useState<FamilyDraft | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editingDraft, setEditingDraft] = useState<FamilyDraft>(EMPTY_DRAFT);
  const [deleteTarget, setDeleteTarget] = useState<ArticleFamily | null>(null);

  const loadFamilies = async () => {
    setLoading(true);
    setError(null);
    try {
      setFamilies(await listInventoryArticleFamilies());
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void loadFamilies();
  }, []);

  const beginEdit = (family: ArticleFamily) => {
    if (!capabilities.canUpdate) return;
    setEditingId(family.id);
    setEditingDraft({
      code: family.code,
      name: family.name,
      description: family.description ?? "",
    });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditingDraft(EMPTY_DRAFT);
  };

  const submitCreate = async () => {
    if (!capabilities.canCreate) return;
    setSaving(true);
    setError(null);
    try {
      await createInventoryArticleFamily({
        code: newDraft?.code ?? "",
        name: newDraft?.name ?? "",
        description: newDraft?.description || null,
      });
      setNewDraft(null);
      await loadFamilies();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const submitEdit = async (familyId: number) => {
    if (!capabilities.canUpdate) return;
    setSaving(true);
    setError(null);
    try {
      await updateInventoryArticleFamily(familyId, {
        code: editingDraft.code,
        name: editingDraft.name,
        description: editingDraft.description || null,
        is_active: true,
      });
      cancelEdit();
      await loadFamilies();
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
      await deactivateInventoryArticleFamily(deleteTarget.id);
      setDeleteTarget(null);
      await loadFamilies();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <ReferenceValueTable
      loading={loading && families.length === 0}
      error={error}
      title={
        <>
          <span className={REF_TABLE_TITLE_CLASS}>{t("synthetic.articleFamilies.title")}</span>
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
            {t("synthetic.articleFamilies.add")}
          </Button>
        ) : null
      }
      emptyLabel={t("synthetic.articleFamilies.empty")}
      showEmpty={families.length === 0 && !newDraft}
      confirm={
        deleteTarget
          ? {
              open: true,
              title: t("synthetic.articleFamilies.deactivateTitle"),
              description: t("synthetic.articleFamilies.deactivateDescription", {
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
            <ReferenceValueTableHeadCell>
              {t("synthetic.articleFamilies.colName")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("editor.colDescription")}</ReferenceValueTableHeadCell>
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
                  placeholder={t("editor.codePlaceholder")}
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
                  placeholder={t("synthetic.articleFamilies.namePlaceholder")}
                  value={newDraft.name}
                  onChange={(e) =>
                    setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), name: e.target.value }))
                  }
                  className="h-7 text-sm"
                />
              </ReferenceValueTableCell>
              <ReferenceValueTableCell>
                <Input
                  placeholder={t("editor.descriptionPlaceholder")}
                  value={newDraft.description}
                  onChange={(e) =>
                    setNewDraft((s) => ({ ...(s ?? EMPTY_DRAFT), description: e.target.value }))
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
                    disabled={saving || !newDraft.code.trim() || !newDraft.name.trim()}
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
          {families.map((family) => {
            const isEditing = editingId === family.id;
            return (
              <ReferenceValueTableRow key={family.id} highlighted={isEditing}>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 bg-surface-1 text-sm"
                      value={editingDraft.code}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, code: e.target.value }))}
                    />
                  ) : (
                    <span className="font-mono text-xs">{family.code}</span>
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.name}
                      onChange={(e) => setEditingDraft((s) => ({ ...s, name: e.target.value }))}
                    />
                  ) : (
                    family.name
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {isEditing ? (
                    <Input
                      className="h-7 text-sm"
                      value={editingDraft.description}
                      onChange={(e) =>
                        setEditingDraft((s) => ({ ...s, description: e.target.value }))
                      }
                    />
                  ) : (
                    <span className="inline-block max-w-[200px] truncate text-xs text-text-muted">
                      {family.description ?? "—"}
                    </span>
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Badge
                    variant={family.is_active === 1 ? "default" : "secondary"}
                    className={REF_TABLE_BADGE_CLASS}
                  >
                    {family.is_active === 1 ? t("editor.statusActive") : t("editor.statusInactive")}
                  </Badge>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell align="right">
                  {capabilities.canUpdate && isEditing ? (
                    <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                      <Button
                        variant="ghost"
                        size="icon"
                        className={refTableIconButtonClass()}
                        onClick={() => void submitEdit(family.id)}
                        disabled={saving || !editingDraft.code.trim() || !editingDraft.name.trim()}
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
                        onClick={() => beginEdit(family)}
                        disabled={saving}
                        aria-label={t("editor.edit")}
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </Button>
                      {capabilities.canDeactivateOrDelete && family.is_active === 1 ? (
                        <Button
                          variant="ghost"
                          size="icon"
                          className={refTableIconButtonClass()}
                          onClick={() => setDeleteTarget(family)}
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
