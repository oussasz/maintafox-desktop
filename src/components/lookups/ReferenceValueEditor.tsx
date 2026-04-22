/**
 * ReferenceValueEditor.tsx
 *
 * Right pane of ReferenceManagerPage. Editable DataTable for reference
 * values within the selected set — inline create, edit, delete with
 * pagination, permission gates, and protected-domain awareness.
 *
 * Phase 2 – Sub-phase 03 – File 02 – Sprint S4 (GAP REF-03).
 */

import {
  AlertTriangle,
  Check,
  ChevronLeft,
  ChevronRight,
  Pencil,
  Plus,
  Trash2,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { PublishReadinessPanel } from "@/components/lookups/PublishReadinessPanel";
import { ReferenceAliasPanel } from "@/components/lookups/ReferenceAliasPanel";
import { ReferenceColorSwatchHex } from "@/components/lookups/ReferenceColorSwatchHex";
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
import { usePermissions } from "@/hooks/use-permissions";
import { isReferenceDomainProtected } from "@/lib/reference-domain-ui";
import { listReferenceValues } from "@/services/reference-service";
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
import type { CreateReferenceValuePayload, ReferenceValue } from "@shared/ipc-types";

// ── Constants ─────────────────────────────────────────────────────────────────

const PAGE_SIZE = 50;

function isSystemReferenceRow(metadataJson: string | null): boolean {
  if (!metadataJson) return false;
  try {
    const o = JSON.parse(metadataJson) as { origin?: string };
    return o.origin === "system";
  } catch {
    return metadataJson.includes('"origin":"system"');
  }
}

// ── Inline edit row state ─────────────────────────────────────────────────────

interface EditRowState {
  code: string;
  label: string;
  description: string;
  parentId: number | null;
}

// ── Component ─────────────────────────────────────────────────────────────────

interface ReferenceValueEditorProps {
  setId: number;
  domainId: number;
}

export function ReferenceValueEditor({ setId, domainId }: ReferenceValueEditorProps) {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();

  // ── Store bindings ───────────────────────────────────────────────────────

  const values = useReferenceGovernanceStore((s) => s.values);
  const valuesLoading = useReferenceGovernanceStore((s) => s.valuesLoading);
  const editingValueId = useReferenceGovernanceStore((s) => s.editingValueId);
  const newValueDraft = useReferenceGovernanceStore((s) => s.newValueDraft);
  const savingValue = useReferenceGovernanceStore((s) => s.savingValue);
  const error = useReferenceGovernanceStore((s) => s.error);
  const loadValues = useReferenceGovernanceStore((s) => s.loadValues);
  const saveValue = useReferenceGovernanceStore((s) => s.saveValue);
  const addValue = useReferenceGovernanceStore((s) => s.addValue);
  const removeValue = useReferenceGovernanceStore((s) => s.removeValue);
  const setEditingValueId = useReferenceGovernanceStore((s) => s.setEditingValueId);
  const setNewValueDraft = useReferenceGovernanceStore((s) => s.setNewValueDraft);

  const domains = useReferenceManagerStore((s) => s.domains);
  const setsMap = useReferenceManagerStore((s) => s.setsMap);
  const loadSetsForDomain = useReferenceManagerStore((s) => s.loadSetsForDomain);

  const domain = domains.find((d) => d.id === domainId);
  const refSet = setsMap[domainId]?.find((s) => s.id === setId);
  const isDraft = refSet?.status === "draft";
  const isProtected = domain ? isReferenceDomainProtected(domain) : false;
  /** System-locked domains (is_extendable = false): catalog is view-only in the UI. */
  const domainLocked = domain ? !domain.is_extendable : false;

  // ── Local state ──────────────────────────────────────────────────────────

  const [page, setPage] = useState(0);
  const [editRow, setEditRow] = useState<EditRowState>({
    code: "",
    label: "",
    description: "",
    parentId: null,
  });
  const [newRow, setNewRow] = useState<EditRowState>({
    code: "",
    label: "",
    description: "",
    parentId: null,
  });
  const [deleteTarget, setDeleteTarget] = useState<ReferenceValue | null>(null);
  const [aliasValueId, setAliasValueId] = useState<number | null>(null);
  const [sortField, setSortField] = useState<"code" | "label" | "is_active">("code");
  const [sortAsc, setSortAsc] = useState(true);
  const [parentCandidates, setParentCandidates] = useState<ReferenceValue[]>([]);
  const parentDomainCodeByChild: Record<string, string> = {
    "EQUIPMENT.FAMILY": "EQUIPMENT.CLASS",
    "EQUIPMENT.SUBFAMILY": "EQUIPMENT.FAMILY",
  };
  const parentDomainCode = domain ? parentDomainCodeByChild[domain.code] : undefined;
  const parentDomain = parentDomainCode
    ? (domains.find((d) => d.code === parentDomainCode) ?? null)
    : null;
  const parentSetId = parentDomain
    ? (setsMap[parentDomain.id]?.find((s) => s.status === "published")?.id ?? null)
    : null;

  // ── Load values on set selection ─────────────────────────────────────────

  useEffect(() => {
    void loadValues(setId);
    setPage(0);
  }, [setId, loadValues]);

  useEffect(() => {
    if (!parentDomain) {
      setParentCandidates([]);
      return;
    }
    if (!setsMap[parentDomain.id]?.length) {
      void loadSetsForDomain(parentDomain.id);
    }
  }, [parentDomain, setsMap, loadSetsForDomain]);

  useEffect(() => {
    if (!parentSetId) {
      setParentCandidates([]);
      return;
    }
    let cancelled = false;
    void listReferenceValues(parentSetId)
      .then((rows) => {
        if (!cancelled) {
          setParentCandidates(rows.filter((r) => r.is_active));
        }
      })
      .catch(() => {
        if (!cancelled) {
          setParentCandidates([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [parentSetId]);

  // ── Sorted + paginated values ────────────────────────────────────────────

  const sortedValues = useMemo(() => {
    const sorted = [...values].sort((a, b) => {
      let cmp = 0;
      if (sortField === "code") cmp = a.code.localeCompare(b.code);
      else if (sortField === "label") cmp = a.label.localeCompare(b.label);
      else cmp = Number(a.is_active) - Number(b.is_active);
      return sortAsc ? cmp : -cmp;
    });
    return sorted;
  }, [values, sortField, sortAsc]);

  const showColorColumn = useMemo(
    () => values.some((v) => v.color_hex != null && String(v.color_hex).trim() !== ""),
    [values],
  );

  const parentValuesById = useMemo(
    () => new Map(parentCandidates.map((v) => [v.id, v])),
    [parentCandidates],
  );

  const totalPages = Math.max(1, Math.ceil(sortedValues.length / PAGE_SIZE));
  const pagedValues = sortedValues.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);

  // ── Inline edit handlers ─────────────────────────────────────────────────

  const startEdit = useCallback(
    (v: ReferenceValue) => {
      setEditingValueId(v.id);
      setEditRow({
        code: v.code,
        label: v.label,
        description: v.description ?? "",
        parentId: v.parent_id ?? null,
      });
    },
    [setEditingValueId],
  );

  const cancelEdit = useCallback(() => {
    setEditingValueId(null);
  }, [setEditingValueId]);

  const commitEdit = useCallback(
    async (valueId: number) => {
      await saveValue(valueId, {
        label: editRow.label,
        description: editRow.description || null,
      });
    },
    [saveValue, editRow],
  );

  // ── New row handlers ─────────────────────────────────────────────────────

  const startNewRow = useCallback(() => {
    setNewValueDraft({});
    setNewRow({ code: "", label: "", description: "", parentId: null });
  }, [setNewValueDraft]);

  const cancelNewRow = useCallback(() => {
    setNewValueDraft(null);
  }, [setNewValueDraft]);

  const commitNewRow = useCallback(async () => {
    if (!newRow.code.trim() || !newRow.label.trim()) return;
    const payload: CreateReferenceValuePayload = {
      set_id: setId,
      ...(newRow.parentId != null ? { parent_id: newRow.parentId } : {}),
      code: newRow.code.trim(),
      label: newRow.label.trim(),
      description: newRow.description.trim() || null,
    };
    await addValue(payload);
  }, [addValue, setId, newRow]);

  // ── Delete handler ───────────────────────────────────────────────────────

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    await removeValue(deleteTarget.id);
    setDeleteTarget(null);
  }, [removeValue, deleteTarget]);

  // ── Sort toggle ──────────────────────────────────────────────────────────

  const toggleSort = (field: "code" | "label" | "is_active") => {
    if (sortField === field) {
      setSortAsc(!sortAsc);
    } else {
      setSortField(field);
      setSortAsc(true);
    }
  };

  // ── Keyboard handler for inline edit rows ────────────────────────────────

  const handleEditKeyDown = useCallback(
    (e: React.KeyboardEvent, valueId: number) => {
      if (e.key === "Enter") {
        e.preventDefault();
        void commitEdit(valueId);
      } else if (e.key === "Escape") {
        cancelEdit();
      }
    },
    [commitEdit, cancelEdit],
  );

  const handleNewKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        void commitNewRow();
      } else if (e.key === "Escape") {
        cancelNewRow();
      }
    },
    [commitNewRow, cancelNewRow],
  );

  // ── Alias panel ──────────────────────────────────────────────────────────

  const aliasValue = aliasValueId ? values.find((v) => v.id === aliasValueId) : null;

  // ── Loading state ────────────────────────────────────────────────────────

  if (valuesLoading && values.length === 0) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* ── Publish readiness (draft sets only) ─────────────────────────── */}
      {isDraft && refSet && <PublishReadinessPanel setId={setId} isProtected={!!isProtected} />}

      {/* ── Header ──────────────────────────────────────────────────────── */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-surface-border">
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-sm font-semibold text-text-primary truncate">{domain?.name}</span>
          {refSet && (
            <Badge
              variant={refSet.status === "published" ? "default" : "secondary"}
              className="text-[10px]"
            >
              v{refSet.version_no} — {refSet.status}
            </Badge>
          )}
          {isProtected && (
            <Badge variant="outline" className="text-[10px] text-status-warning">
              {t("editor.protected")}
            </Badge>
          )}
        </div>

        <div className="flex items-center gap-2">
          <PermissionGate permission="ref.manage">
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5"
              onClick={startNewRow}
              disabled={!!newValueDraft || savingValue || domainLocked}
            >
              <Plus className="h-3.5 w-3.5" />
              {t("editor.addValue")}
            </Button>
          </PermissionGate>
        </div>
      </div>

      {/* ── Error banner ────────────────────────────────────────────────── */}
      {error && (
        <div className="px-4 py-2 bg-red-50 dark:bg-red-950/20 text-sm text-status-danger flex items-center gap-2">
          <AlertTriangle className="h-4 w-4 shrink-0" />
          {error}
        </div>
      )}

      {/* ── Table ───────────────────────────────────────────────────────── */}
      <div className="flex-1 overflow-auto">
        {values.length === 0 && !newValueDraft ? (
          <div className="flex h-full items-center justify-center p-6">
            <div className="text-center space-y-2">
              <p className="text-sm text-text-muted">{t("editor.emptyState")}</p>
            </div>
          </div>
        ) : (
          <table className="w-full text-sm">
            <thead className="sticky top-0 bg-surface-0 border-b border-surface-border z-10">
              <tr>
                <th
                  className="px-3 py-2 text-left font-medium text-text-muted cursor-pointer select-none"
                  onClick={() => toggleSort("code")}
                  onKeyDown={(e) => e.key === "Enter" && toggleSort("code")}
                >
                  {t("editor.colCode")} {sortField === "code" && (sortAsc ? "↑" : "↓")}
                </th>
                <th
                  className="px-3 py-2 text-left font-medium text-text-muted cursor-pointer select-none"
                  onClick={() => toggleSort("label")}
                  onKeyDown={(e) => e.key === "Enter" && toggleSort("label")}
                >
                  {t("editor.colLabel")} {sortField === "label" && (sortAsc ? "↑" : "↓")}
                </th>
                <th className="px-3 py-2 text-left font-medium text-text-muted">
                  {t("editor.colDescription")}
                </th>
                <th className="px-3 py-2 text-left font-medium text-text-muted">
                  {t("editor.colParent")}
                </th>
                {showColorColumn ? (
                  <th className="px-3 py-2 text-left align-middle font-medium text-text-muted">
                    {t("editor.colColor")}
                  </th>
                ) : null}
                <th
                  className="px-3 py-2 text-left font-medium text-text-muted cursor-pointer select-none"
                  onClick={() => toggleSort("is_active")}
                  onKeyDown={(e) => e.key === "Enter" && toggleSort("is_active")}
                >
                  {t("editor.colStatus")} {sortField === "is_active" && (sortAsc ? "↑" : "↓")}
                </th>
                <th className="px-3 py-2 text-right align-middle font-medium text-text-muted">
                  {t("editor.colActions")}
                </th>
              </tr>
            </thead>
            <tbody>
              {/* ── New value row ──────────────────────────────────────── */}
              {newValueDraft && (
                <tr className="bg-primary/5 border-b border-surface-border">
                  <td className="px-3 py-1.5">
                    <Input
                      value={newRow.code}
                      onChange={(e) => setNewRow({ ...newRow, code: e.target.value })}
                      onKeyDown={handleNewKeyDown}
                      placeholder={t("editor.codePlaceholder")}
                      className="h-7 text-sm"
                      autoFocus
                    />
                  </td>
                  <td className="px-3 py-1.5">
                    <Input
                      value={newRow.label}
                      onChange={(e) => setNewRow({ ...newRow, label: e.target.value })}
                      onKeyDown={handleNewKeyDown}
                      placeholder={t("editor.labelPlaceholder")}
                      className="h-7 text-sm"
                    />
                  </td>
                  <td className="px-3 py-1.5">
                    <Input
                      value={newRow.description}
                      onChange={(e) => setNewRow({ ...newRow, description: e.target.value })}
                      onKeyDown={handleNewKeyDown}
                      placeholder={t("editor.descriptionPlaceholder")}
                      className="h-7 text-sm"
                    />
                  </td>
                  <td className="px-3 py-1.5">
                    {parentSetId ? (
                      <select
                        value={newRow.parentId == null ? "" : String(newRow.parentId)}
                        onChange={(e) =>
                          setNewRow({
                            ...newRow,
                            parentId: e.target.value ? Number(e.target.value) : null,
                          })
                        }
                        className="h-7 w-full rounded-md border border-surface-border bg-surface-0 px-2 text-sm"
                      >
                        <option value="">{t("editor.none")}</option>
                        {parentCandidates.map((p) => (
                          <option key={p.id} value={p.id}>
                            {p.code} — {p.label}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <span className="text-text-muted">—</span>
                    )}
                  </td>
                  {showColorColumn ? (
                    <td className="px-3 py-1.5 align-middle text-text-muted text-sm">—</td>
                  ) : null}
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
                        onClick={() => void commitNewRow()}
                        disabled={savingValue || !newRow.code.trim() || !newRow.label.trim()}
                      >
                        <Check className="h-3.5 w-3.5 text-status-success" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        className={refTableIconButtonClass()}
                        onClick={cancelNewRow}
                      >
                        <X className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </td>
                </tr>
              )}

              {/* ── Value rows ─────────────────────────────────────────── */}
              {pagedValues.map((v) => {
                const isEditing = editingValueId === v.id;
                const parentValue = v.parent_id
                  ? (values.find((p) => p.id === v.parent_id) ??
                    parentValuesById.get(v.parent_id) ??
                    null)
                  : null;
                const isSystemRow = isSystemReferenceRow(v.metadata_json);
                const showPencil = can("ref.manage") && !domainLocked;
                const showTrash = can("ref.manage") && !domainLocked && !isSystemRow;

                return (
                  <tr
                    key={v.id}
                    className={`border-b border-surface-border hover:bg-surface-1 ${
                      isEditing ? "bg-primary/5" : ""
                    } ${aliasValueId === v.id ? "ring-1 ring-inset ring-primary/30" : ""}`}
                  >
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input value={editRow.code} disabled className="h-7 text-sm bg-surface-1" />
                      ) : (
                        <span className="font-mono text-xs">{v.code}</span>
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          value={editRow.label}
                          onChange={(e) => setEditRow({ ...editRow, label: e.target.value })}
                          onKeyDown={(e) => handleEditKeyDown(e, v.id)}
                          className="h-7 text-sm"
                          autoFocus
                        />
                      ) : (
                        <div className="flex min-w-0 flex-col gap-0.5 sm:flex-row sm:items-center sm:gap-2">
                          <span className="min-w-0">{v.label}</span>
                          {can("ref.manage") ? (
                            <button
                              type="button"
                              className="shrink-0 text-left text-xs font-medium text-primary underline-offset-2 hover:underline"
                              onClick={() => setAliasValueId(aliasValueId === v.id ? null : v.id)}
                            >
                              {t("editor.aliases")}
                            </button>
                          ) : null}
                        </div>
                      )}
                    </td>
                    <td className="px-3 py-1.5">
                      {isEditing ? (
                        <Input
                          value={editRow.description}
                          onChange={(e) => setEditRow({ ...editRow, description: e.target.value })}
                          onKeyDown={(e) => handleEditKeyDown(e, v.id)}
                          className="h-7 text-sm"
                        />
                      ) : (
                        <span className="text-text-muted text-xs truncate max-w-[200px] inline-block">
                          {v.description ?? "—"}
                        </span>
                      )}
                    </td>
                    <td className="px-3 py-1.5 text-text-muted text-xs">
                      {parentValue ? parentValue.code : "—"}
                    </td>
                    {showColorColumn ? (
                      <td className="px-3 py-1.5 align-middle">
                        <ReferenceColorSwatchHex color={v.color_hex} />
                      </td>
                    ) : null}
                    <td className="px-3 py-1.5 align-middle">
                      <Badge
                        variant={v.is_active ? "default" : "secondary"}
                        className="text-[10px]"
                      >
                        {v.is_active ? t("editor.statusActive") : t("editor.statusInactive")}
                      </Badge>
                    </td>
                    <td className="px-3 py-1.5 text-right align-middle">
                      {isEditing ? (
                        <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                          <Button
                            variant="ghost"
                            size="icon"
                            className={refTableIconButtonClass()}
                            onClick={() => void commitEdit(v.id)}
                            disabled={savingValue}
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
                          {showPencil ? (
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              aria-label={t("editor.edit")}
                              onClick={() => startEdit(v)}
                            >
                              <Pencil className="h-3.5 w-3.5" />
                            </Button>
                          ) : null}
                          {showTrash ? (
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon"
                              className={refTableIconButtonClass()}
                              aria-label={t("editor.deactivate")}
                              onClick={() => setDeleteTarget(v)}
                            >
                              <Trash2 className="h-3.5 w-3.5" />
                            </Button>
                          ) : null}
                        </div>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {/* ── Alias panel (expanded below table) ──────────────────────────── */}
      {aliasValue && (
        <div className="border-t border-surface-border">
          <ReferenceAliasPanel value={aliasValue} onClose={() => setAliasValueId(null)} />
        </div>
      )}

      {/* ── Pagination ──────────────────────────────────────────────────── */}
      {totalPages > 1 && (
        <div className="flex items-center justify-between px-4 py-2 border-t border-surface-border">
          <span className="text-xs text-text-muted">
            {t("editor.pageInfo", {
              current: page + 1,
              total: totalPages,
              count: sortedValues.length,
            })}
          </span>
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={() => setPage(Math.max(0, page - 1))}
              disabled={page === 0}
            >
              <ChevronLeft className="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
              disabled={page >= totalPages - 1}
            >
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>
      )}

      {/* ── Delete confirmation dialog ──────────────────────────────────── */}
      <Dialog open={deleteTarget !== null} onOpenChange={(open) => !open && setDeleteTarget(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("editor.deleteTitle")}</DialogTitle>
            <DialogDescription>
              {t("editor.deleteDescription", { code: deleteTarget?.code })}
            </DialogDescription>
          </DialogHeader>
          {!deleteTarget?.is_active && (
            <p className="text-xs text-text-muted">{t("editor.alreadyInactive")}</p>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)} disabled={savingValue}>
              {t("editor.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => void confirmDelete()}
              disabled={savingValue}
            >
              {t("editor.deactivate")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
