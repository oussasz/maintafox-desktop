/**
 * ReferenceAliasPanel.tsx
 *
 * Sub-panel within ReferenceValueEditor — displays and manages aliases for
 * a selected reference value. Supports inline CRUD with preferred-per-locale
 * radio behavior and alias type enforcement.
 *
 * Phase 2 – Sub-phase 03 – File 03 – Sprint S4 (GAP REF-06).
 */

import { Check, Pencil, Plus, Star, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { mfTable } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  createReferenceAlias,
  deleteReferenceAlias,
  listReferenceAliases,
  updateReferenceAlias,
} from "@/services/reference-service";
import { referenceErrorText } from "@/stores/reference-error-text";
import type { ReferenceAlias, ReferenceValue } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// ── Types ─────────────────────────────────────────────────────────────────────

const ALIAS_TYPES = ["synonym", "legacy", "import", "abbreviation"] as const;
const LOCALES = ["fr", "en"] as const;

interface NewAliasRow {
  alias_label: string;
  locale: string;
  alias_type: string;
  is_preferred: boolean;
}

interface EditAliasRow {
  alias_label: string;
  locale: string;
  alias_type: string;
  is_preferred: boolean;
}

// ── Component ─────────────────────────────────────────────────────────────────

interface ReferenceAliasPanelProps {
  value: ReferenceValue;
  /** Value mutation allowed by governance + ref.manage (from parent capabilities). */
  canMutate: boolean;
  onClose: () => void;
}

export function ReferenceAliasPanel({ value, canMutate, onClose }: ReferenceAliasPanelProps) {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canMutateAliases = canMutate && can(P.REF_MANAGE);

  const [aliases, setAliases] = useState<ReferenceAlias[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [editRow, setEditRow] = useState<EditAliasRow>({
    alias_label: "",
    locale: "fr",
    alias_type: "synonym",
    is_preferred: false,
  });
  const [newRow, setNewRow] = useState<NewAliasRow | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ReferenceAlias | null>(null);

  // ── Load aliases ─────────────────────────────────────────────────────────

  const loadAliases = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await listReferenceAliases(value.id);
      setAliases(result);
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setLoading(false);
    }
  }, [value.id]);

  useEffect(() => {
    void loadAliases();
  }, [loadAliases]);

  // ── CRUD handlers ────────────────────────────────────────────────────────

  const startAdd = () => {
    setNewRow({ alias_label: "", locale: "fr", alias_type: "synonym", is_preferred: false });
    setEditingId(null);
  };

  const cancelAdd = () => setNewRow(null);

  const commitAdd = async () => {
    if (!newRow || !newRow.alias_label.trim()) return;
    setSaving(true);
    try {
      await createReferenceAlias({
        reference_value_id: value.id,
        alias_label: newRow.alias_label.trim(),
        locale: newRow.locale,
        alias_type: newRow.alias_type,
        is_preferred: newRow.is_preferred,
      });
      setNewRow(null);
      await loadAliases();
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setSaving(false);
    }
  };

  const startEdit = (a: ReferenceAlias) => {
    setEditingId(a.id);
    setEditRow({
      alias_label: a.alias_label,
      locale: a.locale,
      alias_type: a.alias_type,
      is_preferred: a.is_preferred,
    });
    setNewRow(null);
  };

  const cancelEdit = () => setEditingId(null);

  const commitEdit = async (aliasId: number) => {
    setSaving(true);
    try {
      await updateReferenceAlias(aliasId, {
        alias_label: editRow.alias_label,
        locale: editRow.locale,
        alias_type: editRow.alias_type,
        is_preferred: editRow.is_preferred,
      });
      setEditingId(null);
      await loadAliases();
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setSaving(true);
    try {
      await deleteReferenceAlias(deleteTarget.id);
      setDeleteTarget(null);
      await loadAliases();
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setSaving(false);
    }
  };

  // ── Render ───────────────────────────────────────────────────────────────

  return (
    <div className="px-4 py-3 space-y-3 max-h-[280px] overflow-auto">
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-medium text-text-primary">
          {t("alias.title", { code: value.code, label: value.label })}
        </h4>
        <div className="flex items-center gap-2">
          {canMutateAliases && (
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5 h-10 text-xs"
              onClick={startAdd}
              disabled={!!newRow || saving}
            >
              <Plus className="h-3 w-3" />
              {t("alias.add")}
            </Button>
          )}
          <Button
            aria-label={String(t("editor.cancel"))}
            title={String(t("editor.cancel"))}
            variant="ghost"
            size="icon"
            className="h-9 w-9"
            onClick={onClose}
          >
            <X className="h-4 w-4" aria-hidden />
          </Button>
        </div>
      </div>

      {error && (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {error}
        </p>
      )}

      {loading ? (
        <LoadingState className="py-4" />
      ) : aliases.length === 0 && !newRow ? (
        <p className="text-xs text-text-muted italic">{t("alias.empty")}</p>
      ) : (
        <table className="w-full text-xs">
          <thead>
            <tr className={cn("border-b border-surface-border", mfTable.header)}>
              <th className="px-2 py-1 text-start">{t("alias.colAlias")}</th>
              <th className="px-2 py-1 text-start">{t("alias.colLocale")}</th>
              <th className="px-2 py-1 text-start">{t("alias.colType")}</th>
              <th className="px-2 py-1 text-start">{t("alias.colPreferred")}</th>
              <th className="px-2 py-1 text-right align-middle font-medium text-text-muted">
                {t("alias.colActions")}
              </th>
            </tr>
          </thead>
          <tbody>
            {/* New alias row */}
            {newRow && (
              <tr className="bg-primary/5 border-b border-surface-border">
                <td className="px-2 py-1">
                  <Input
                    value={newRow.alias_label}
                    onChange={(e) => setNewRow({ ...newRow, alias_label: e.target.value })}
                    placeholder={t("alias.labelPlaceholder")}
                    className="h-6 text-xs"
                    autoFocus
                    onKeyDown={(e) => {
                      if (e.key === "Enter") void commitAdd();
                      if (e.key === "Escape") cancelAdd();
                    }}
                  />
                </td>
                <td className="px-2 py-1">
                  <Select
                    value={newRow.locale}
                    onValueChange={(v) => setNewRow({ ...newRow, locale: v })}
                  >
                    <SelectTrigger className="h-6 text-xs w-16">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {LOCALES.map((l) => (
                        <SelectItem key={l} value={l}>
                          {l.toUpperCase()}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </td>
                <td className="px-2 py-1">
                  <Select
                    value={newRow.alias_type}
                    onValueChange={(v) => setNewRow({ ...newRow, alias_type: v })}
                  >
                    <SelectTrigger className="h-6 text-xs w-24">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {ALIAS_TYPES.map((at) => (
                        <SelectItem key={at} value={at}>
                          {at}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </td>
                <td className="px-2 py-1">
                  <Button
                    aria-label={String(t("alias.preferred"))}
                    title={String(t("alias.preferred"))}
                    variant="ghost"
                    size="icon"
                    className="h-5 w-5"
                    onClick={() => setNewRow({ ...newRow, is_preferred: !newRow.is_preferred })}
                  >
                    <Star
                      className={`h-3 w-3 ${newRow.is_preferred ? "fill-tone-warning-fg text-tone-warning-fg" : "text-text-muted"}`}
                    />
                  </Button>
                </td>
                <td className="px-2 py-1 text-right align-middle">
                  <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                    <Button
                      aria-label={String(t("editor.save"))}
                      title={String(t("editor.save"))}
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      onClick={() => void commitAdd()}
                      disabled={saving || !newRow.alias_label.trim()}
                    >
                      <Check className="h-3.5 w-3.5 text-status-success" />
                    </Button>
                    <Button
                      aria-label={String(t("editor.cancel"))}
                      title={String(t("editor.cancel"))}
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      onClick={cancelAdd}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </td>
              </tr>
            )}

            {/* Existing aliases */}
            {aliases.map((a) => {
              const isEditing = editingId === a.id;
              return (
                <tr
                  key={a.id}
                  className={`border-b border-surface-border ${isEditing ? "bg-primary/5" : ""}`}
                >
                  <td className="px-2 py-1">
                    {isEditing ? (
                      <Input
                        value={editRow.alias_label}
                        onChange={(e) => setEditRow({ ...editRow, alias_label: e.target.value })}
                        className="h-6 text-xs"
                        autoFocus
                        onKeyDown={(e) => {
                          if (e.key === "Enter") void commitEdit(a.id);
                          if (e.key === "Escape") cancelEdit();
                        }}
                      />
                    ) : (
                      a.alias_label
                    )}
                  </td>
                  <td className="px-2 py-1">
                    {isEditing ? (
                      <Select
                        value={editRow.locale}
                        onValueChange={(v) => setEditRow({ ...editRow, locale: v })}
                      >
                        <SelectTrigger className="h-6 text-xs w-16">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {LOCALES.map((l) => (
                            <SelectItem key={l} value={l}>
                              {l.toUpperCase()}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <span className="uppercase">{a.locale}</span>
                    )}
                  </td>
                  <td className="px-2 py-1">
                    {isEditing ? (
                      <Select
                        value={editRow.alias_type}
                        onValueChange={(v) => setEditRow({ ...editRow, alias_type: v })}
                      >
                        <SelectTrigger className="h-6 text-xs w-24">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {ALIAS_TYPES.map((at) => (
                            <SelectItem key={at} value={at}>
                              {at}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    ) : (
                      <Badge variant="outline" className="text-[9px]">
                        {a.alias_type}
                      </Badge>
                    )}
                  </td>
                  <td className="px-2 py-1">
                    {isEditing ? (
                      <Button
                        aria-label={String(t("alias.preferred"))}
                        title={String(t("alias.preferred"))}
                        variant="ghost"
                        size="icon"
                        className="h-5 w-5"
                        onClick={() =>
                          setEditRow({ ...editRow, is_preferred: !editRow.is_preferred })
                        }
                      >
                        <Star
                          className={`h-3 w-3 ${editRow.is_preferred ? "fill-tone-warning-fg text-tone-warning-fg" : "text-text-muted"}`}
                        />
                      </Button>
                    ) : a.is_preferred ? (
                      <Star className="h-3 w-3 fill-tone-warning-fg text-tone-warning-fg" />
                    ) : null}
                  </td>
                  <td className="px-2 py-1 text-right align-middle">
                    {isEditing ? (
                      <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                        <Button
                          aria-label={String(t("editor.save"))}
                          title={String(t("editor.save"))}
                          variant="ghost"
                          size="icon"
                          className={refTableIconButtonClass()}
                          onClick={() => void commitEdit(a.id)}
                          disabled={saving}
                        >
                          <Check className="h-3.5 w-3.5 text-status-success" />
                        </Button>
                        <Button
                          aria-label={String(t("editor.cancel"))}
                          title={String(t("editor.cancel"))}
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
                        {canMutateAliases && (
                          <>
                            <Button
                              title={String(t("editor.edit"))}
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              onClick={() => startEdit(a)}
                              aria-label={t("alias.edit")}
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                            <Button
                              title={String(t("alias.delete"))}
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              onClick={() => setDeleteTarget(a)}
                              disabled={saving}
                              aria-label={t("alias.delete")}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          </>
                        )}
                      </div>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      <Dialog
        open={deleteTarget != null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("alias.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("alias.deleteDescription", { label: deleteTarget?.alias_label ?? "" })}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={saving}>
              {t("editor.cancel")}
            </Button>
            <Button variant="destructive" onClick={() => void handleDelete()} disabled={saving}>
              {t("alias.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
