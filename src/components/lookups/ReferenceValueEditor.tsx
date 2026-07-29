/**
 * ReferenceValueEditor — container for real reference domains.
 * Chrome lives in ReferenceValueTable; governance wiring stays here.
 */

import { Check, ChevronLeft, ChevronRight, Pencil, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { PublishReadinessPanel } from "@/components/lookups/PublishReadinessPanel";
import { ReferenceAliasPanel } from "@/components/lookups/ReferenceAliasPanel";
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
import { SchedulePatternDialog } from "@/components/lookups/SchedulePatternPanel";
import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  REF_TABLE_BADGE_CLASS,
  REF_TABLE_EMPTY_ACTIONS_MARK,
  REF_TABLE_TITLE_CLASS,
  refTableHeaderAddButtonClass,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import { resolveReferenceValueColumns } from "@/components/lookups/reference-value-columns";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { usePermissions } from "@/hooks/use-permissions";
import { useReferenceCapabilities } from "@/hooks/use-reference-capabilities";
import {
  governanceCategoryLabelKey,
  preferredWorkingSet,
  publishedReadOnlyBannerKey,
} from "@/lib/reference-governance-ui";
import {
  createDraftReferenceSet,
  discardDraftReferenceSet,
  listReferenceValues,
  moveReferenceValueParent,
} from "@/services/reference-service";
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
import { toErrorMessage } from "@/utils/errors";
import type { CreateReferenceValuePayload, ReferenceValue } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

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

interface EditRowState {
  code: string;
  label: string;
  description: string;
  parentId: number | null;
}

interface ReferenceValueEditorProps {
  setId: number;
  domainId: number;
}

export function ReferenceValueEditor({ setId, domainId }: ReferenceValueEditorProps) {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();

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
  const setValueActive = useReferenceGovernanceStore((s) => s.setValueActive);
  const setEditingValueId = useReferenceGovernanceStore((s) => s.setEditingValueId);
  const setNewValueDraft = useReferenceGovernanceStore((s) => s.setNewValueDraft);

  const domains = useReferenceManagerStore((s) => s.domains);
  const setsMap = useReferenceManagerStore((s) => s.setsMap);
  const loadSetsForDomain = useReferenceManagerStore((s) => s.loadSetsForDomain);
  const selectSet = useReferenceManagerStore((s) => s.selectSet);

  const domain = domains.find((d) => d.id === domainId);
  const refSet = setsMap[domainId]?.find((s) => s.id === setId);
  const isDraft = refSet?.status === "draft";
  const setStatus = refSet?.status ?? null;
  const { caps } = useReferenceCapabilities(domainId, setId, setStatus);
  const canManage = can(P.REF_MANAGE);
  const canCreateValue = Boolean(caps?.can_create_value && canManage);
  const canUpdateValue = Boolean(caps?.can_update_value && canManage);
  const canDeactivateValue = Boolean(caps?.can_deactivate_value && canManage);
  const canCreateDraftSet = Boolean(caps?.can_create_draft_set && canManage);
  const canDiscardDraftSet = Boolean(caps?.can_discard_draft_set && canManage);
  const canShowPublishPanel = Boolean(caps?.can_publish && isDraft && refSet);
  const showReadOnlyBanner =
    caps != null &&
    !caps.can_create_value &&
    (setStatus === "published" || setStatus === "superseded");
  const isAnalyticalProtected = Boolean(caps?.requires_analytical_protection);

  const [page, setPage] = useState(0);
  const [creatingDraft, setCreatingDraft] = useState(false);
  const [discardingDraft, setDiscardingDraft] = useState(false);
  const [discardConfirmOpen, setDiscardConfirmOpen] = useState(false);
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
  const [patternValueId, setPatternValueId] = useState<number | null>(null);
  const [sortField, setSortField] = useState<"code" | "label" | "is_active">("code");
  const [sortAsc, setSortAsc] = useState(true);
  const [parentCandidates, setParentCandidates] = useState<ReferenceValue[]>([]);
  const columnFlags = useMemo(() => resolveReferenceValueColumns(domain), [domain]);
  const { showParent, showSchedule, crossDomainParentCode, sameSetHierarchy } = columnFlags;
  const parentDomain = crossDomainParentCode
    ? (domains.find((d) => d.code === crossDomainParentCode) ?? null)
    : null;
  const parentSetId = parentDomain
    ? (setsMap[parentDomain.id]?.find((s) => s.status === "published")?.id ?? null)
    : null;

  useEffect(() => {
    void loadValues(setId);
    setPage(0);
  }, [setId, loadValues]);

  useEffect(() => {
    if (!showParent) {
      setParentCandidates([]);
      return;
    }
    if (sameSetHierarchy) {
      setParentCandidates(values.filter((r) => r.is_active));
      return;
    }
    if (!parentDomain) {
      setParentCandidates([]);
      return;
    }
    if (!setsMap[parentDomain.id]?.length) {
      void loadSetsForDomain(parentDomain.id);
    }
  }, [showParent, sameSetHierarchy, values, parentDomain, setsMap, loadSetsForDomain]);

  useEffect(() => {
    if (!showParent || sameSetHierarchy || !parentSetId) {
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
  }, [showParent, sameSetHierarchy, parentSetId]);

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

  const startEdit = useCallback(
    (v: ReferenceValue) => {
      setEditingValueId(v.id);
      setEditRow({
        code: v.code,
        label: v.label,
        description: v.description ?? "",
        parentId: v.parent_id ?? null,
      });
      setAliasValueId(null);
      if (showSchedule) {
        setPatternValueId(v.id);
      }
    },
    [setEditingValueId, showSchedule],
  );

  const cancelEdit = useCallback(() => {
    setEditingValueId(null);
    setPatternValueId(null);
  }, [setEditingValueId]);

  const commitEdit = useCallback(
    async (valueId: number) => {
      const existing = values.find((v) => v.id === valueId);
      try {
        await saveValue(valueId, {
          label: editRow.label,
          description: editRow.description || null,
        });
        if (showParent && existing && (existing.parent_id ?? null) !== (editRow.parentId ?? null)) {
          const moved = await moveReferenceValueParent(valueId, editRow.parentId);
          useReferenceGovernanceStore.setState((s) => ({
            values: s.values.map((v) => (v.id === valueId ? moved : v)),
          }));
        }
        setPatternValueId(null);
      } catch {
        // Error already stored in governance store
      }
    },
    [saveValue, editRow, values, showParent],
  );

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

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return;
    await removeValue(deleteTarget.id);
    setDeleteTarget(null);
  }, [removeValue, deleteTarget]);

  const handleCreateDraft = useCallback(async () => {
    if (!canCreateDraftSet || creatingDraft) return;
    setCreatingDraft(true);
    try {
      const draft = await createDraftReferenceSet(domainId);
      await loadSetsForDomain(domainId);
      selectSet(draft.id, domainId);
    } catch (err) {
      useReferenceGovernanceStore.setState({ error: toErrorMessage(err) });
    } finally {
      setCreatingDraft(false);
    }
  }, [canCreateDraftSet, creatingDraft, domainId, loadSetsForDomain, selectSet]);

  const handleDiscardDraft = useCallback(async () => {
    if (!canDiscardDraftSet || discardingDraft) return;
    setDiscardingDraft(true);
    try {
      await discardDraftReferenceSet(setId);
      setDiscardConfirmOpen(false);
      await loadSetsForDomain(domainId);
      const sets = useReferenceManagerStore.getState().setsMap[domainId] ?? [];
      const published = preferredWorkingSet(sets);
      if (published) {
        selectSet(published.id, domainId);
      } else {
        useReferenceManagerStore.setState({
          selectedDomainId: domainId,
          selectedSetId: null,
        });
      }
    } catch (err) {
      useReferenceGovernanceStore.setState({ error: toErrorMessage(err) });
    } finally {
      setDiscardingDraft(false);
    }
  }, [canDiscardDraftSet, discardingDraft, setId, domainId, loadSetsForDomain, selectSet]);

  const toggleSort = (field: "code" | "label" | "is_active") => {
    if (sortField === field) {
      setSortAsc(!sortAsc);
    } else {
      setSortField(field);
      setSortAsc(true);
    }
  };

  const handleEditKeyDown = useCallback(
    (e: KeyboardEvent, valueId: number) => {
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
    (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        void commitNewRow();
      } else if (e.key === "Escape") {
        cancelNewRow();
      }
    },
    [commitNewRow, cancelNewRow],
  );

  const aliasValue = aliasValueId ? values.find((v) => v.id === aliasValueId) : null;
  const patternValue = patternValueId ? values.find((v) => v.id === patternValueId) : null;

  const banner = (
    <>
      {showReadOnlyBanner ? (
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-surface-border bg-surface-1 px-4 py-3">
          <p className="text-sm text-text-secondary">
            {String(t(publishedReadOnlyBannerKey(setStatus ?? "published", canCreateDraftSet)))}
          </p>
          {canCreateDraftSet ? (
            <Button
              variant="default"
              size="sm"
              className="gap-1.5 shrink-0"
              disabled={creatingDraft}
              onClick={() => void handleCreateDraft()}
            >
              <Plus className="h-3.5 w-3.5" />
              {creatingDraft
                ? t("governance.readOnly.creatingDraft")
                : t("governance.readOnly.createDraft")}
            </Button>
          ) : null}
        </div>
      ) : null}
      {caps?.is_read_only && !showReadOnlyBanner ? (
        <div className="border-b border-surface-border bg-surface-1 px-4 py-3 text-sm text-text-secondary">
          {t("governance.readOnly.systemCatalog")}
        </div>
      ) : null}
    </>
  );

  return (
    <>
      <ReferenceValueTable
        loading={valuesLoading && values.length === 0}
        error={error}
        aboveHeader={
          canShowPublishPanel && refSet ? (
            <PublishReadinessPanel setId={setId} isProtected={isAnalyticalProtected} />
          ) : null
        }
        title={
          <>
            <span className={REF_TABLE_TITLE_CLASS}>{domain?.name}</span>
            {refSet ? (
              <Badge
                variant={refSet.status === "published" ? "default" : "secondary"}
                className={REF_TABLE_BADGE_CLASS}
              >
                v{refSet.version_no} —{" "}
                {t(`browser.status.${refSet.status}` as "browser.status.draft", {
                  defaultValue: refSet.status,
                })}
              </Badge>
            ) : null}
            {caps?.category ? (
              <Badge variant="outline" className={REF_TABLE_BADGE_CLASS}>
                {String(t(governanceCategoryLabelKey(caps.category)))}
              </Badge>
            ) : null}
            {isAnalyticalProtected ? (
              <Badge variant="outline" className={`${REF_TABLE_BADGE_CLASS} text-status-warning`}>
                {t("editor.protected")}
              </Badge>
            ) : null}
          </>
        }
        toolbar={
          <>
            {canDiscardDraftSet ? (
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-status-danger border-status-danger/40 hover:bg-status-danger/10"
                disabled={discardingDraft}
                onClick={() => setDiscardConfirmOpen(true)}
              >
                <Trash2 className="h-3.5 w-3.5" />
                {discardingDraft
                  ? t("governance.readOnly.discardingDraft")
                  : t("governance.readOnly.discardDraft")}
              </Button>
            ) : null}
            {canCreateValue ? (
              <Button
                variant="outline"
                size="sm"
                className={refTableHeaderAddButtonClass()}
                onClick={startNewRow}
                disabled={!!newValueDraft || savingValue}
              >
                <Plus className="h-3.5 w-3.5" />
                {t("editor.addValue")}
              </Button>
            ) : null}
          </>
        }
        banner={banner}
        emptyLabel={t("editor.emptyState")}
        showEmpty={values.length === 0 && !newValueDraft}
        footer={
          <>
            {aliasValue ? (
              <div className="border-t border-surface-border">
                <ReferenceAliasPanel
                  value={aliasValue}
                  canMutate={canUpdateValue}
                  onClose={() => setAliasValueId(null)}
                />
              </div>
            ) : null}
            {totalPages > 1 ? (
              <div className="flex items-center justify-between border-t border-surface-border px-4 py-2">
                <span className="text-xs text-text-muted">
                  {t("editor.pageInfo", {
                    start: page * PAGE_SIZE + 1,
                    end: Math.min((page + 1) * PAGE_SIZE, sortedValues.length),
                    total: sortedValues.length,
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
            ) : null}
          </>
        }
        confirm={
          deleteTarget
            ? {
                open: true,
                title: t("editor.deleteTitle"),
                description: t("editor.deleteDescription", { label: deleteTarget.label }),
                confirmLabel: t("editor.deactivate"),
                cancelLabel: t("editor.cancel"),
                destructive: true,
                busy: savingValue,
                hint: !deleteTarget.is_active ? t("editor.alreadyInactive") : null,
                onOpenChange: (open) => {
                  if (!open) setDeleteTarget(null);
                },
                onConfirm: () => void confirmDelete(),
              }
            : discardConfirmOpen
              ? {
                  open: true,
                  title: t("governance.readOnly.discardDraftTitle"),
                  description: t("governance.readOnly.discardDraftDescription"),
                  confirmLabel: discardingDraft
                    ? t("governance.readOnly.discardingDraft")
                    : t("governance.readOnly.discardDraft"),
                  cancelLabel: t("editor.cancel"),
                  destructive: true,
                  busy: discardingDraft,
                  onOpenChange: setDiscardConfirmOpen,
                  onConfirm: () => void handleDiscardDraft(),
                }
              : null
        }
      >
        <ReferenceValueTableGrid>
          <ReferenceValueTableHead>
            <tr>
              <ReferenceValueTableHeadCell sortable onSort={() => toggleSort("code")}>
                {t("editor.colCode")} {sortField === "code" && (sortAsc ? "↑" : "↓")}
              </ReferenceValueTableHeadCell>
              <ReferenceValueTableHeadCell sortable onSort={() => toggleSort("label")}>
                {t("editor.colLabel")} {sortField === "label" && (sortAsc ? "↑" : "↓")}
              </ReferenceValueTableHeadCell>
              <ReferenceValueTableHeadCell>
                {t("editor.colDescription")}
              </ReferenceValueTableHeadCell>
              {showSchedule ? (
                <ReferenceValueTableHeadCell>
                  {t("schedulePattern.column")}
                </ReferenceValueTableHeadCell>
              ) : null}
              {showParent ? (
                <ReferenceValueTableHeadCell>{t("editor.colParent")}</ReferenceValueTableHeadCell>
              ) : null}
              {showColorColumn ? (
                <ReferenceValueTableHeadCell>{t("editor.colColor")}</ReferenceValueTableHeadCell>
              ) : null}
              <ReferenceValueTableHeadCell sortable onSort={() => toggleSort("is_active")}>
                {t("editor.colStatus")} {sortField === "is_active" && (sortAsc ? "↑" : "↓")}
              </ReferenceValueTableHeadCell>
              <ReferenceValueTableHeadCell align="right">
                {t("editor.colActions")}
              </ReferenceValueTableHeadCell>
            </tr>
          </ReferenceValueTableHead>
          <ReferenceValueTableBody>
            {newValueDraft ? (
              <ReferenceValueTableRow highlighted>
                <ReferenceValueTableCell>
                  <Input
                    value={newRow.code}
                    onChange={(e) => setNewRow({ ...newRow, code: e.target.value })}
                    onKeyDown={handleNewKeyDown}
                    placeholder={t("editor.codePlaceholder")}
                    className="h-7 text-sm"
                    autoFocus
                  />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Input
                    value={newRow.label}
                    onChange={(e) => setNewRow({ ...newRow, label: e.target.value })}
                    onKeyDown={handleNewKeyDown}
                    placeholder={t("editor.labelPlaceholder")}
                    className="h-7 text-sm"
                  />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Input
                    value={newRow.description}
                    onChange={(e) => setNewRow({ ...newRow, description: e.target.value })}
                    onKeyDown={handleNewKeyDown}
                    placeholder={t("editor.descriptionPlaceholder")}
                    className="h-7 text-sm"
                  />
                </ReferenceValueTableCell>
                {showSchedule ? (
                  <ReferenceValueTableCell className="text-xs text-text-muted">
                    —
                  </ReferenceValueTableCell>
                ) : null}
                {showParent ? (
                  <ReferenceValueTableCell>
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
                  </ReferenceValueTableCell>
                ) : null}
                {showColorColumn ? (
                  <ReferenceValueTableCell>
                    <span className="text-sm text-text-muted">—</span>
                  </ReferenceValueTableCell>
                ) : null}
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
                </ReferenceValueTableCell>
              </ReferenceValueTableRow>
            ) : null}

            {pagedValues.map((v) => {
              const isEditing = editingValueId === v.id;
              const parentValue = v.parent_id
                ? (values.find((p) => p.id === v.parent_id) ??
                  parentValuesById.get(v.parent_id) ??
                  null)
                : null;
              const isSystemRow = isSystemReferenceRow(v.metadata_json);
              const showPencil = canUpdateValue;
              const showTrash = canDeactivateValue && !isSystemRow && v.is_active;

              return (
                <ReferenceValueTableRow
                  key={v.id}
                  highlighted={isEditing}
                  className={
                    aliasValueId === v.id || patternValueId === v.id
                      ? "ring-1 ring-inset ring-primary/30"
                      : undefined
                  }
                >
                  <ReferenceValueTableCell>
                    {isEditing ? (
                      <Input value={editRow.code} disabled className="h-7 bg-surface-1 text-sm" />
                    ) : (
                      <span className="font-mono text-xs">{v.code}</span>
                    )}
                  </ReferenceValueTableCell>
                  <ReferenceValueTableCell>
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
                        {canUpdateValue ? (
                          <button
                            type="button"
                            className="shrink-0 text-left text-xs font-medium text-primary underline-offset-2 hover:underline"
                            onClick={() => {
                              setAliasValueId(aliasValueId === v.id ? null : v.id);
                              if (aliasValueId !== v.id) setPatternValueId(null);
                            }}
                          >
                            {t("editor.aliases")}
                          </button>
                        ) : null}
                      </div>
                    )}
                  </ReferenceValueTableCell>
                  <ReferenceValueTableCell>
                    {isEditing ? (
                      <Input
                        value={editRow.description}
                        onChange={(e) => setEditRow({ ...editRow, description: e.target.value })}
                        onKeyDown={(e) => handleEditKeyDown(e, v.id)}
                        className="h-7 text-sm"
                      />
                    ) : (
                      <span className="inline-block max-w-[200px] truncate text-xs text-text-muted">
                        {v.description ?? "—"}
                      </span>
                    )}
                  </ReferenceValueTableCell>
                  {showSchedule ? (
                    <ReferenceValueTableCell className="text-xs text-text-muted">
                      —
                    </ReferenceValueTableCell>
                  ) : null}
                  {showParent ? (
                    <ReferenceValueTableCell className="text-xs text-text-muted">
                      {isEditing ? (
                        <select
                          value={editRow.parentId == null ? "" : String(editRow.parentId)}
                          onChange={(e) =>
                            setEditRow({
                              ...editRow,
                              parentId: e.target.value ? Number(e.target.value) : null,
                            })
                          }
                          className="h-7 w-full rounded-md border border-surface-border bg-surface-0 px-2 text-sm"
                        >
                          <option value="">{t("editor.none")}</option>
                          {parentCandidates
                            .filter((p) => p.id !== v.id)
                            .map((p) => (
                              <option key={p.id} value={p.id}>
                                {p.code} — {p.label}
                              </option>
                            ))}
                        </select>
                      ) : parentValue ? (
                        parentValue.code
                      ) : (
                        "—"
                      )}
                    </ReferenceValueTableCell>
                  ) : null}
                  {showColorColumn ? (
                    <ReferenceValueTableCell>
                      <ReferenceColorSwatchHex color={v.color_hex} />
                    </ReferenceValueTableCell>
                  ) : null}
                  <ReferenceValueTableCell>
                    <div className="flex items-center gap-2">
                      <Switch
                        checked={v.is_active}
                        onCheckedChange={(next) => void setValueActive(v.id, next)}
                        disabled={savingValue || !canDeactivateValue || isSystemRow}
                        aria-label={t("editor.colStatus")}
                      />
                      <span className="text-sm text-text-muted">
                        {v.is_active ? t("editor.statusActive") : t("editor.statusInactive")}
                      </span>
                    </div>
                  </ReferenceValueTableCell>
                  <ReferenceValueTableCell align="right">
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
                    ) : showPencil || showTrash ? (
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
                    ) : (
                      <span className="text-xs text-text-muted">
                        {REF_TABLE_EMPTY_ACTIONS_MARK}
                      </span>
                    )}
                  </ReferenceValueTableCell>
                </ReferenceValueTableRow>
              );
            })}
          </ReferenceValueTableBody>
        </ReferenceValueTableGrid>
      </ReferenceValueTable>

      <SchedulePatternDialog
        value={patternValue ?? null}
        open={patternValue != null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPatternValueId(null);
        }}
        canMutate={canUpdateValue}
      />
    </>
  );
}
