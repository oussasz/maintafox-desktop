/**
 * ReferenceValueEditor — container for real reference domains.
 * Chrome lives in ReferenceValueTable; governance wiring stays here.
 *
 * What the user may do comes only from the governance capabilities (IPC) combined with the
 * `ref.manage` / `ref.publish` permissions. Whenever an action is not available, the pane says why.
 */

import {
  Check,
  ChevronLeft,
  ChevronRight,
  Download,
  FolderOpen,
  Lock,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
  Upload,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import {
  AbsenceBehaviourSelect,
  AbsenceBehaviourText,
  CriticalTagRemovalWarning,
  removesCriticalTag,
} from "@/components/lookups/AbsenceBehaviourPicker";
import {
  MaintenanceNatureSelect,
  MaintenanceNatureText,
  useMaintenanceNatures,
} from "@/components/lookups/MaintenanceNaturePicker";
import {
  EMPTY_UNIT_EDIT,
  UnitEditCells,
  type UnitEditState,
  UnitHeadCells,
  UnitReadCells,
  baseUnitCandidates,
  isUnitToReview,
  unitEditComplete,
  unitEditStateFrom,
} from "@/components/lookups/MeasureUnitColumns";
import {
  buildPlantUnitMetadata,
  parsePositiveNumber,
  useMeasureDimensions,
} from "@/components/lookups/MeasureUnitPicker";
import {
  EMPTY_PERMIT_RULE_EDIT,
  type PermitRuleEditState,
  PermitRuleEditCells,
  PermitRuleHeadCells,
  PermitRuleReadCells,
  buildPermitRuleMetadata,
  permitRuleComplete,
  permitRuleStateFrom,
  unusablePermitRules,
  usePermitRuleOptions,
} from "@/components/lookups/PermitRuleColumns";
import { PublishReadinessPanel } from "@/components/lookups/PublishReadinessPanel";
import { ReferenceAliasPanel } from "@/components/lookups/ReferenceAliasPanel";
import { ReferenceColorSwatchHex } from "@/components/lookups/ReferenceColorSwatchHex";
import { ReferenceImportWizard } from "@/components/lookups/ReferenceImportWizard";
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
import { buildReferenceCsv } from "@/components/lookups/reference-csv";
import {
  REFERENCE_SET_STATUS_TONE,
  isWorkingCatalogCategory,
  pendingCandidateSet,
  referenceSetStatusLabelKey,
} from "@/components/lookups/reference-set-ui";
import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  REF_TABLE_BADGE_CLASS,
  REF_TABLE_EMPTY_ACTIONS_MARK,
  REF_TABLE_TITLE_CLASS,
  refTableDisabledActionTitle,
  refTableHeaderAddButtonClass,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import { resolveReferenceValueColumns } from "@/components/lookups/reference-value-columns";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { mfInput } from "@/design-system/tokens";
import { toneOf } from "@/design-system/tone";
import { isExportCancelled, saveBlobWithDialog } from "@/export";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useReferenceCapabilities } from "@/hooks/use-reference-capabilities";
import { governanceCategoryLabelKey } from "@/lib/reference-governance-ui";
import { cn } from "@/lib/utils";
import {
  createDraftReferenceSet,
  discardDraftReferenceSet,
  exportRefDomainSet,
  listReferenceValues,
  moveReferenceValueParent,
} from "@/services/reference-service";
import { pushAppToast } from "@/store/app-toast-store";
import { referenceErrorText } from "@/stores/reference-error-text";
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
import type {
  CreateReferenceValuePayload,
  ReferencePublishResult,
  ReferenceValue,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const PAGE_SIZE = 50;

/** Product-seeded value (`metadata_json.origin === "system"`). */
export function isSystemReferenceRow(metadataJson: string | null): boolean {
  if (!metadataJson) return false;
  try {
    const o = JSON.parse(metadataJson) as { origin?: unknown };
    return o.origin === "system";
  } catch {
    return false;
  }
}

interface EditRowState {
  code: string;
  label: string;
  description: string;
  parentId: number | null;
  /** EN 13306 nature code (`semantic_tag`) of a request type; null = corrective fallback. */
  natureTag: string | null;
}

interface ReferenceValueEditorProps {
  setId: number;
  domainId: number;
}

type SortField = "code" | "label" | "is_active";

export function ReferenceValueEditor({ setId, domainId }: ReferenceValueEditorProps) {
  const { t } = useTranslation("reference");
  /** Translation of a key computed at run time (status / category). */
  const label = useCallback((key: string) => String(t(key as "page.title")), [t]);
  const { can } = usePermissions();
  const { formatDate } = useFormatters();

  const values = useReferenceGovernanceStore((s) => s.values);
  const valuesLoading = useReferenceGovernanceStore((s) => s.valuesLoading);
  const valuesError = useReferenceGovernanceStore((s) => s.valuesError);
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
  const domainSets = useMemo(() => setsMap[domainId] ?? [], [setsMap, domainId]);
  const refSet = domainSets.find((s) => s.id === setId);
  const setStatus = refSet?.status ?? null;
  const {
    caps,
    error: capsError,
    reload: reloadCaps,
  } = useReferenceCapabilities(domainId, setId, setStatus);
  const canManage = can(P.REF_MANAGE);
  const canCreateValue = Boolean(caps?.can_create_value && canManage);
  const canUpdateValue = Boolean(caps?.can_update_value && canManage);
  const canDeactivateValue = Boolean(caps?.can_deactivate_value && canManage);
  const canCreateDraftSet = Boolean(caps?.can_create_draft_set && canManage);
  const canDiscardDraftSet = Boolean(caps?.can_discard_draft_set && canManage);
  const isCandidate = setStatus === "draft" || setStatus === "validated";
  const showPublishPanel = Boolean(caps?.can_publish && isCandidate && refSet);
  const systemValuesLocked = Boolean(caps?.system_values_locked);
  const isAnalyticalProtected = Boolean(caps?.requires_analytical_protection);
  const workingCatalog = isWorkingCatalogCategory(caps?.category ?? domain?.governance_category);
  const pendingCandidate = pendingCandidateSet(domainSets);
  const otherCandidate =
    pendingCandidate && pendingCandidate.id !== setId ? pendingCandidate : null;
  const canImport = canCreateValue && setStatus === "draft";

  const [page, setPage] = useState(0);
  const [creatingDraft, setCreatingDraft] = useState(false);
  const [discardConfirmOpen, setDiscardConfirmOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [editRow, setEditRow] = useState<EditRowState>({
    code: "",
    label: "",
    description: "",
    parentId: null,
    natureTag: null,
  });
  const [newRow, setNewRow] = useState<EditRowState>({
    code: "",
    label: "",
    description: "",
    parentId: null,
    natureTag: null,
  });
  const [deleteTarget, setDeleteTarget] = useState<ReferenceValue | null>(null);
  const [aliasValueId, setAliasValueId] = useState<number | null>(null);
  const [patternValueId, setPatternValueId] = useState<number | null>(null);
  const [sortField, setSortField] = useState<SortField>("code");
  const [sortAsc, setSortAsc] = useState(true);
  const [parentCandidates, setParentCandidates] = useState<ReferenceValue[]>([]);
  const columnFlags = useMemo(() => resolveReferenceValueColumns(domain), [domain]);
  const {
    showParent,
    showSchedule,
    showNature,
    showAbsenceBehaviour,
    showUnit,
    showPermitRule,
    crossDomainParentCode,
    sameSetHierarchy,
  } = columnFlags;
  // The nature (request types) and the absence behaviour (absence types) are both `semantic_tag`.
  const showSemanticTag = showNature || showAbsenceBehaviour;
  // Permit requirement rules (PERMIT.REQUIREMENT_RULES): class / work type → permit type.
  const permitRuleOptions = usePermitRuleOptions(showPermitRule);
  const [editRule, setEditRule] = useState<PermitRuleEditState>(EMPTY_PERMIT_RULE_EDIT);
  const [newRule, setNewRule] = useState<PermitRuleEditState>(EMPTY_PERMIT_RULE_EDIT);
  /** Rules the conversion cannot use (not applied when a request becomes a work order). */
  const unusableRules = useMemo(
    () => (showPermitRule ? unusablePermitRules(values, permitRuleOptions) : []),
    [showPermitRule, values, permitRuleOptions],
  );
  const natures = useMaintenanceNatures(showNature);
  // Units of measure (MEASURE.UNITS): unit fields of the row being edited / created, the bases a
  // plant unit may name (standard units of this version) and the "to review" filter.
  const dimensions = useMeasureDimensions(showUnit);
  const [editUnit, setEditUnit] = useState<UnitEditState>(EMPTY_UNIT_EDIT);
  const [newUnit, setNewUnit] = useState<UnitEditState>(EMPTY_UNIT_EDIT);
  const [reviewOnly, setReviewOnly] = useState(false);
  const unitBases = useMemo(() => (showUnit ? baseUnitCandidates(values) : []), [showUnit, values]);
  const reviewCount = useMemo(
    () => (showUnit ? values.filter(isUnitToReview).length : 0),
    [showUnit, values],
  );
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
        // The global toast reports the failure; the parent column then shows codes only.
        if (!cancelled) {
          setParentCandidates([]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [showParent, sameSetHierarchy, parentSetId]);

  const sortedValues = useMemo(() => {
    const shown = showUnit && reviewOnly ? values.filter(isUnitToReview) : values;
    return [...shown].sort((a, b) => {
      let cmp = 0;
      if (sortField === "code") cmp = a.code.localeCompare(b.code);
      else if (sortField === "label") cmp = a.label.localeCompare(b.label);
      else cmp = Number(a.is_active) - Number(b.is_active);
      return sortAsc ? cmp : -cmp;
    });
  }, [values, sortField, sortAsc, showUnit, reviewOnly]);

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
  const hasSystemRows = useMemo(
    () => values.some((v) => isSystemReferenceRow(v.metadata_json)),
    [values],
  );

  const startEdit = useCallback(
    (v: ReferenceValue) => {
      setEditingValueId(v.id);
      setEditRow({
        code: v.code,
        label: v.label,
        description: v.description ?? "",
        parentId: v.parent_id ?? null,
        natureTag: v.semantic_tag ?? null,
      });
      if (showUnit) setEditUnit(unitEditStateFrom(v));
      if (showPermitRule) setEditRule(permitRuleStateFrom(v));
      setAliasValueId(null);
      if (showSchedule) {
        setPatternValueId(v.id);
      }
    },
    [setEditingValueId, showSchedule, showUnit, showPermitRule],
  );

  const cancelEdit = useCallback(() => {
    setEditingValueId(null);
    setPatternValueId(null);
  }, [setEditingValueId]);

  const commitEdit = useCallback(
    async (valueId: number) => {
      const existing = values.find((v) => v.id === valueId);
      try {
        if (showPermitRule && existing) {
          if (!permitRuleComplete(editRule)) return;
          await saveValue(valueId, {
            label: editRow.label,
            description: editRow.description || null,
            metadata_json: buildPermitRuleMetadata(existing.metadata_json, editRule),
          });
          return;
        }
        if (showUnit && existing) {
          // A plant unit: its base first (the server checks base and factor together on save),
          // then the labels, symbol and factor.
          const factor = parsePositiveNumber(editUnit.factor);
          if (factor == null || editUnit.baseId == null) return;
          if ((existing.parent_id ?? null) !== editUnit.baseId) {
            const moved = await moveReferenceValueParent(valueId, editUnit.baseId);
            useReferenceGovernanceStore.setState((s) => ({
              values: s.values.map((v) => (v.id === valueId ? moved : v)),
            }));
          }
          await saveValue(valueId, {
            label: editRow.label,
            description: editRow.description || null,
            metadata_json: buildPlantUnitMetadata(existing.metadata_json, {
              symbol: editUnit.symbol,
              labelEn: editUnit.labelEn,
              baseFactor: factor,
            }),
          });
          return;
        }
        await saveValue(valueId, {
          label: editRow.label,
          description: editRow.description || null,
          // "" clears the nature on the server (corrective fallback); null would leave it. An
          // absence type without a behaviour is refused by the server (required).
          ...(showSemanticTag ? { semantic_tag: editRow.natureTag ?? "" } : {}),
        });
        if (showParent && existing && (existing.parent_id ?? null) !== (editRow.parentId ?? null)) {
          const moved = await moveReferenceValueParent(valueId, editRow.parentId);
          useReferenceGovernanceStore.setState((s) => ({
            values: s.values.map((v) => (v.id === valueId ? moved : v)),
          }));
        }
        setPatternValueId(null);
      } catch {
        // The store keeps the translated error (shown above the table) and the row stays in edit.
      }
    },
    [
      saveValue,
      editRow,
      values,
      showParent,
      showSemanticTag,
      showUnit,
      editUnit,
      showPermitRule,
      editRule,
    ],
  );

  const startNewRow = useCallback(() => {
    setNewValueDraft({});
    setNewRow({ code: "", label: "", description: "", parentId: null, natureTag: null });
    setNewUnit(EMPTY_UNIT_EDIT);
    setNewRule(EMPTY_PERMIT_RULE_EDIT);
  }, [setNewValueDraft]);

  const cancelNewRow = useCallback(() => {
    setNewValueDraft(null);
  }, [setNewValueDraft]);

  const commitNewRow = useCallback(async () => {
    if (!newRow.code.trim() || !newRow.label.trim()) return;
    if (showPermitRule) {
      if (!permitRuleComplete(newRule)) return;
      await addValue({
        set_id: setId,
        code: newRow.code.trim(),
        label: newRow.label.trim(),
        description: newRow.description.trim() || null,
        metadata_json: buildPermitRuleMetadata(null, newRule),
      });
      return;
    }
    if (showUnit) {
      // A plant unit: base standard unit (same set) and factor; the dimension is the base's.
      const factor = parsePositiveNumber(newUnit.factor);
      if (factor == null || newUnit.baseId == null || !newUnit.symbol.trim()) return;
      await addValue({
        set_id: setId,
        parent_id: newUnit.baseId,
        code: newRow.code.trim(),
        label: newRow.label.trim(),
        description: newRow.description.trim() || null,
        metadata_json: buildPlantUnitMetadata(null, {
          symbol: newUnit.symbol,
          labelEn: newUnit.labelEn || newRow.label,
          baseFactor: factor,
        }),
      });
      return;
    }
    const payload: CreateReferenceValuePayload = {
      set_id: setId,
      ...(newRow.parentId != null ? { parent_id: newRow.parentId } : {}),
      code: newRow.code.trim(),
      label: newRow.label.trim(),
      description: newRow.description.trim() || null,
      ...(showSemanticTag && newRow.natureTag ? { semantic_tag: newRow.natureTag } : {}),
    };
    await addValue(payload);
  }, [addValue, setId, newRow, showSemanticTag, showUnit, newUnit, showPermitRule, newRule]);

  const confirmDeactivate = useCallback(async () => {
    if (!deleteTarget) return;
    await removeValue(deleteTarget.id);
    if (useReferenceGovernanceStore.getState().error) {
      // Keep the dialog open; the failure is shown above the table and by the toast.
      throw new Error(useReferenceGovernanceStore.getState().error ?? "");
    }
    setDeleteTarget(null);
  }, [removeValue, deleteTarget]);

  const openSet = useCallback(
    async (targetSetId: number) => {
      await loadSetsForDomain(domainId);
      selectSet(targetSetId, domainId);
    },
    [loadSetsForDomain, selectSet, domainId],
  );

  const handleCreateDraft = useCallback(async () => {
    if (!canCreateDraftSet || creatingDraft) return;
    setCreatingDraft(true);
    try {
      const draft = await createDraftReferenceSet(domainId);
      await openSet(draft.id);
    } catch (err) {
      useReferenceGovernanceStore.setState({ error: referenceErrorText(err) });
      // A draft created elsewhere in the meantime: show it instead of failing silently.
      await loadSetsForDomain(domainId);
    } finally {
      setCreatingDraft(false);
    }
  }, [canCreateDraftSet, creatingDraft, domainId, openSet, loadSetsForDomain]);

  const handleDiscardDraft = useCallback(async () => {
    await discardDraftReferenceSet(setId);
    await loadSetsForDomain(domainId);
    const sets = useReferenceManagerStore.getState().setsMap[domainId] ?? [];
    const published = sets
      .filter((s) => s.status === "published")
      .sort((a, b) => b.version_no - a.version_no)[0];
    if (published) {
      selectSet(published.id, domainId);
    } else {
      useReferenceManagerStore.setState({ selectedDomainId: domainId, selectedSetId: null });
    }
  }, [setId, domainId, loadSetsForDomain, selectSet]);

  const handlePublished = useCallback(
    (result: ReferencePublishResult) => {
      pushAppToast({
        title: t("publish.publishedToast", { version: result.set.version_no }),
        variant: "success",
      });
      void openSet(result.set.id);
      reloadCaps();
    },
    [openSet, reloadCaps, t],
  );

  const handleExport = useCallback(async () => {
    if (exporting) return;
    setExporting(true);
    try {
      const exported = await exportRefDomainSet(setId);
      const csv = buildReferenceCsv(exported.rows);
      const fileName = `${exported.domain.code}_v${exported.set.version_no}.csv`;
      const path = await saveBlobWithDialog(
        new Blob([csv], { type: "text/csv;charset=utf-8" }),
        fileName,
        { title: t("export.dialogTitle"), filters: [{ name: "CSV", extensions: ["csv"] }] },
      );
      pushAppToast({
        title: t("export.done", { count: exported.rows.length }),
        description: path,
        variant: "success",
      });
    } catch (err) {
      if (!isExportCancelled(err)) {
        useReferenceGovernanceStore.setState({ error: referenceErrorText(err) });
      }
    } finally {
      setExporting(false);
    }
  }, [exporting, setId, t]);

  const toggleSort = (field: SortField) => {
    if (sortField === field) {
      setSortAsc(!sortAsc);
    } else {
      setSortField(field);
      setSortAsc(true);
    }
  };
  const sortDirection = (field: SortField) =>
    sortField === field ? (sortAsc ? "ascending" : "descending") : null;

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

  const parentLabel = (p: ReferenceValue) => `${p.label} (${p.code})`;

  // ── Why values cannot be changed here (one visible reason) ─────────────────
  let readOnlyReason: string | null = null;
  let readOnlyAction: {
    label: string;
    icon: typeof Plus;
    onClick: () => void;
    busy?: boolean;
  } | null = null;
  if (!caps && capsError) {
    // Without the governance answer no change is offered: say so and let the user retry.
    readOnlyReason = t("governance.capsError");
    readOnlyAction = { label: t("page.retry"), icon: RefreshCw, onClick: reloadCaps };
  } else if (caps) {
    if (caps.is_read_only) {
      readOnlyReason = t("governance.readOnly.systemCatalog");
    } else if (
      !caps.can_create_value &&
      (setStatus === "published" || setStatus === "superseded")
    ) {
      if (otherCandidate) {
        readOnlyReason = t("governance.readOnly.candidateInProgress", {
          version: otherCandidate.version_no,
        });
        readOnlyAction = {
          label: t("governance.readOnly.openCandidate", { version: otherCandidate.version_no }),
          icon: FolderOpen,
          onClick: () => void openSet(otherCandidate.id),
        };
      } else if (canCreateDraftSet) {
        readOnlyReason =
          setStatus === "superseded"
            ? t("governance.readOnly.superseded")
            : t("governance.readOnly.published");
        readOnlyAction = {
          label: creatingDraft
            ? t("governance.readOnly.creatingDraft")
            : t("governance.readOnly.createDraft"),
          icon: Plus,
          onClick: () => void handleCreateDraft(),
          busy: creatingDraft,
        };
      } else if (caps.can_create_draft_set && !canManage) {
        readOnlyReason = t("governance.readOnly.noManagePermission");
      } else {
        readOnlyReason =
          setStatus === "superseded"
            ? t("governance.readOnly.supersededProtected")
            : t("governance.readOnly.publishedProtected");
      }
    } else if (setStatus === "validated") {
      readOnlyReason = t("governance.readOnly.validated");
    } else if (!canManage && (caps.can_create_value || caps.can_update_value)) {
      readOnlyReason = t("governance.readOnly.noManagePermission");
    }
  }

  const banner = (
    <>
      {readOnlyReason ? (
        <div className="flex flex-wrap items-center justify-between gap-3 border-b border-surface-border bg-surface-1 px-4 py-3">
          <p className="text-sm text-text-secondary">{readOnlyReason}</p>
          {readOnlyAction ? (
            <Button
              type="button"
              className="h-11 shrink-0 gap-1.5"
              disabled={readOnlyAction.busy}
              onClick={readOnlyAction.onClick}
            >
              <readOnlyAction.icon className="h-4 w-4" aria-hidden />
              {readOnlyAction.label}
            </Button>
          ) : null}
        </div>
      ) : null}
      {systemValuesLocked && hasSystemRows && !caps?.is_read_only ? (
        <p className="flex items-center gap-1.5 border-b border-surface-border px-4 py-2 text-xs text-text-muted">
          <Lock className="h-3.5 w-3.5 shrink-0" aria-hidden />
          {t("governance.systemValuesLocked")}
        </p>
      ) : null}
    </>
  );

  const versionChip = refSet ? (
    workingCatalog ? (
      <StatusChip label={t("browser.workingCatalog")} tone="info" />
    ) : (
      <>
        <span className="text-xs text-text-secondary">
          {t("editor.version", { version: refSet.version_no })}
        </span>
        <StatusChip
          label={label(referenceSetStatusLabelKey(refSet.status))}
          tone={toneOf(REFERENCE_SET_STATUS_TONE, refSet.status)}
        />
        {refSet.published_at && refSet.status !== "draft" ? (
          <span className="text-xs text-text-muted">
            {t("editor.publishedOn", { date: formatDate(refSet.published_at) })}
          </span>
        ) : null}
      </>
    )
  ) : null;

  return (
    <>
      <ReferenceValueTable
        loading={valuesLoading && values.length === 0}
        loadError={valuesError && values.length === 0 ? valuesError : null}
        onRetry={() => void loadValues(setId)}
        error={valuesError ? null : error}
        aboveHeader={
          <>
            {showPermitRule ? (
              <p className="px-1 pb-2 text-xs text-text-secondary">{t("permitRules.intro")}</p>
            ) : null}
            {showPermitRule && unusableRules.length > 0 ? (
              <p
                role="alert"
                className="mb-2 rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
              >
                {t("permitRules.unusableBanner", {
                  count: unusableRules.length,
                  codes: unusableRules.map((v) => v.code).join(", "),
                })}
              </p>
            ) : null}
            {showPermitRule && permitRuleOptions.error != null ? (
              <ErrorState
                className="mb-2 py-4"
                error={permitRuleOptions.error}
                title={t("permitRules.optionsLoadError")}
                onRetry={permitRuleOptions.reload}
              />
            ) : null}
            {showPublishPanel && refSet ? (
              <PublishReadinessPanel
                setId={setId}
                setStatus={refSet.status}
                isProtected={isAnalyticalProtected}
                onPublished={handlePublished}
              />
            ) : null}
          </>
        }
        title={
          <>
            <span className={REF_TABLE_TITLE_CLASS}>{domain?.name}</span>
            {versionChip}
            {caps?.category ? (
              <Badge variant="outline" className={REF_TABLE_BADGE_CLASS}>
                {label(governanceCategoryLabelKey(caps.category))}
              </Badge>
            ) : null}
            {isAnalyticalProtected ? (
              <StatusChip label={t("editor.protected")} tone="warning" icon={Lock} />
            ) : null}
          </>
        }
        toolbar={
          <>
            {showUnit && reviewCount > 0 ? (
              <Button
                type="button"
                variant={reviewOnly ? "default" : "outline"}
                className="h-11 gap-1.5"
                aria-pressed={reviewOnly}
                onClick={() => {
                  setReviewOnly(!reviewOnly);
                  setPage(0);
                }}
                title={t("units.toReviewHint")}
              >
                {t("units.reviewFilter", { count: reviewCount })}
              </Button>
            ) : null}
            {canDiscardDraftSet && setStatus === "draft" ? (
              <Button
                type="button"
                variant="outline"
                className="h-11 gap-1.5"
                onClick={() => setDiscardConfirmOpen(true)}
              >
                <Trash2 className="h-4 w-4" aria-hidden />
                {t("governance.readOnly.discardDraft")}
              </Button>
            ) : null}
            <Button
              type="button"
              variant="outline"
              className="h-11 gap-1.5"
              onClick={() => void handleExport()}
              disabled={exporting || values.length === 0}
              title={values.length === 0 ? t("export.nothingToExport") : undefined}
            >
              <Download className="h-4 w-4" aria-hidden />
              {t("export.action")}
            </Button>
            {canImport ? (
              <Button
                type="button"
                variant="outline"
                className="h-11 gap-1.5"
                onClick={() => setImportOpen(true)}
              >
                <Upload className="h-4 w-4" aria-hidden />
                {t("page.import")}
              </Button>
            ) : null}
            {canCreateValue ? (
              <Button
                type="button"
                className={refTableHeaderAddButtonClass("h-11")}
                onClick={startNewRow}
                disabled={!!newValueDraft || savingValue}
                title={newValueDraft ? t("editor.addInProgress") : undefined}
              >
                <Plus className="h-4 w-4" aria-hidden />
                {t("editor.addValue")}
              </Button>
            ) : null}
          </>
        }
        banner={banner}
        emptyLabel={canCreateValue ? t("editor.emptyStateCanAdd") : t("editor.emptyState")}
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
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-11 w-11"
                    aria-label={t("editor.previousPage")}
                    onClick={() => setPage(Math.max(0, page - 1))}
                    disabled={page === 0}
                  >
                    <ChevronLeft className="h-4 w-4" aria-hidden />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="h-11 w-11"
                    aria-label={t("editor.nextPage")}
                    onClick={() => setPage(Math.min(totalPages - 1, page + 1))}
                    disabled={page >= totalPages - 1}
                  >
                    <ChevronRight className="h-4 w-4" aria-hidden />
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
                destructive: true,
                hint: !deleteTarget.is_active ? t("editor.alreadyInactive") : null,
                onOpenChange: (open) => {
                  if (!open) setDeleteTarget(null);
                },
                onConfirm: confirmDeactivate,
              }
            : discardConfirmOpen
              ? {
                  open: true,
                  title: t("governance.readOnly.discardDraftTitle"),
                  description: t("governance.readOnly.discardDraftDescription"),
                  confirmLabel: t("governance.readOnly.discardDraft"),
                  destructive: true,
                  onOpenChange: setDiscardConfirmOpen,
                  onConfirm: handleDiscardDraft,
                }
              : null
        }
      >
        <ReferenceValueTableGrid>
          <ReferenceValueTableHead>
            <tr>
              <ReferenceValueTableHeadCell
                sortable
                sortDirection={sortDirection("code")}
                onSort={() => toggleSort("code")}
              >
                {t("editor.colCode")}
              </ReferenceValueTableHeadCell>
              <ReferenceValueTableHeadCell
                sortable
                sortDirection={sortDirection("label")}
                onSort={() => toggleSort("label")}
              >
                {t("editor.colLabel")}
              </ReferenceValueTableHeadCell>
              <ReferenceValueTableHeadCell>
                {t("editor.colDescription")}
              </ReferenceValueTableHeadCell>
              {showUnit ? <UnitHeadCells /> : null}
              {showPermitRule ? <PermitRuleHeadCells /> : null}
              {showSchedule ? (
                <ReferenceValueTableHeadCell>
                  {t("schedulePattern.column")}
                </ReferenceValueTableHeadCell>
              ) : null}
              {showParent ? (
                <ReferenceValueTableHeadCell>{t("editor.colParent")}</ReferenceValueTableHeadCell>
              ) : null}
              {showNature ? (
                <ReferenceValueTableHeadCell>
                  {t("maintenanceNature.column")}
                </ReferenceValueTableHeadCell>
              ) : null}
              {showAbsenceBehaviour ? (
                <ReferenceValueTableHeadCell>
                  {t("absenceBehaviour.column")}
                </ReferenceValueTableHeadCell>
              ) : null}
              {showColorColumn ? (
                <ReferenceValueTableHeadCell>{t("editor.colColor")}</ReferenceValueTableHeadCell>
              ) : null}
              <ReferenceValueTableHeadCell
                sortable
                sortDirection={sortDirection("is_active")}
                onSort={() => toggleSort("is_active")}
              >
                {t("editor.colStatus")}
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
                    aria-label={t("editor.colCode")}
                    className="h-9 text-sm"
                    autoFocus
                  />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Input
                    value={newRow.label}
                    onChange={(e) => setNewRow({ ...newRow, label: e.target.value })}
                    onKeyDown={handleNewKeyDown}
                    placeholder={t("editor.labelPlaceholder")}
                    aria-label={t("editor.colLabel")}
                    className="h-9 text-sm"
                  />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <Input
                    value={newRow.description}
                    onChange={(e) => setNewRow({ ...newRow, description: e.target.value })}
                    onKeyDown={handleNewKeyDown}
                    placeholder={t("editor.descriptionPlaceholder")}
                    aria-label={t("editor.colDescription")}
                    className="h-9 text-sm"
                  />
                </ReferenceValueTableCell>
                {showUnit ? (
                  <UnitEditCells
                    state={newUnit}
                    onChange={setNewUnit}
                    candidates={unitBases}
                    dimensions={dimensions}
                    onKeyDown={handleNewKeyDown}
                  />
                ) : null}
                {showPermitRule ? (
                  <PermitRuleEditCells
                    state={newRule}
                    onChange={setNewRule}
                    options={permitRuleOptions}
                  />
                ) : null}
                {showSchedule ? (
                  <ReferenceValueTableCell className="text-xs text-text-muted">
                    {t("schedulePattern.afterCreate")}
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
                      aria-label={t("editor.colParent")}
                      className={cn(mfInput.nativeSelect, "h-9")}
                    >
                      <option value="">{t("editor.none")}</option>
                      {parentCandidates.map((p) => (
                        <option key={p.id} value={p.id}>
                          {parentLabel(p)}
                        </option>
                      ))}
                    </select>
                  </ReferenceValueTableCell>
                ) : null}
                {showNature ? (
                  <ReferenceValueTableCell>
                    <MaintenanceNatureSelect
                      value={newRow.natureTag}
                      natures={natures}
                      onChange={(natureTag) => setNewRow({ ...newRow, natureTag })}
                    />
                  </ReferenceValueTableCell>
                ) : null}
                {showAbsenceBehaviour ? (
                  <ReferenceValueTableCell>
                    <AbsenceBehaviourSelect
                      value={newRow.natureTag}
                      onChange={(natureTag) => setNewRow({ ...newRow, natureTag })}
                    />
                  </ReferenceValueTableCell>
                ) : null}
                {showColorColumn ? <ReferenceValueTableCell>{null}</ReferenceValueTableCell> : null}
                <ReferenceValueTableCell>
                  <StatusChip label={t("editor.statusNew")} tone="info" />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell align="right">
                  <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      aria-label={t("editor.save")}
                      title={t("editor.save")}
                      onClick={() => void commitNewRow()}
                      disabled={
                        savingValue ||
                        !newRow.code.trim() ||
                        !newRow.label.trim() ||
                        (showUnit && !unitEditComplete(newUnit)) ||
                        (showPermitRule && !permitRuleComplete(newRule))
                      }
                    >
                      <Check className="h-4 w-4 text-status-success" aria-hidden />
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      aria-label={t("editor.cancel")}
                      title={t("editor.cancel")}
                      onClick={cancelNewRow}
                    >
                      <X className="h-4 w-4" aria-hidden />
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
              const lockedRow = isSystemRow && systemValuesLocked;
              const showPencil = canUpdateValue && !lockedRow;
              const showTrash = canDeactivateValue && !isSystemRow && v.is_active;
              const switchReason = refTableDisabledActionTitle({
                isSystem: isSystemRow,
                missingPermission: !canDeactivateValue,
                t,
              });

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
                      <Input
                        value={editRow.code}
                        disabled
                        aria-label={t("editor.colCode")}
                        title={t("editor.codeImmutable")}
                        className="h-9 bg-surface-1 text-sm"
                      />
                    ) : (
                      <span className="inline-flex items-center gap-1.5">
                        {isSystemRow ? (
                          <Lock
                            className="h-3.5 w-3.5 shrink-0 text-text-muted"
                            aria-label={t("editor.systemValue")}
                          />
                        ) : null}
                        <span className="font-mono text-xs">{v.code}</span>
                      </span>
                    )}
                  </ReferenceValueTableCell>
                  <ReferenceValueTableCell>
                    {isEditing ? (
                      <Input
                        value={editRow.label}
                        onChange={(e) => setEditRow({ ...editRow, label: e.target.value })}
                        onKeyDown={(e) => handleEditKeyDown(e, v.id)}
                        aria-label={t("editor.colLabel")}
                        className="h-9 text-sm"
                        autoFocus
                      />
                    ) : (
                      <div className="flex min-w-0 flex-col gap-0.5 sm:flex-row sm:items-center sm:gap-2">
                        <span className="min-w-0">{v.label}</span>
                        {canUpdateValue ? (
                          <button
                            type="button"
                            className="min-h-9 shrink-0 rounded text-left text-xs font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                            aria-expanded={aliasValueId === v.id}
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
                        aria-label={t("editor.colDescription")}
                        className="h-9 text-sm"
                      />
                    ) : (
                      <span
                        className="inline-block max-w-[240px] truncate text-xs text-text-muted"
                        title={v.description ?? undefined}
                      >
                        {v.description ?? "—"}
                      </span>
                    )}
                  </ReferenceValueTableCell>
                  {showUnit ? (
                    isEditing ? (
                      <UnitEditCells
                        state={editUnit}
                        onChange={setEditUnit}
                        candidates={unitBases.filter((b) => b.id !== v.id)}
                        dimensions={dimensions}
                        onKeyDown={(e) => handleEditKeyDown(e, v.id)}
                      />
                    ) : (
                      <UnitReadCells value={v} values={values} dimensions={dimensions} />
                    )
                  ) : null}
                  {showPermitRule ? (
                    isEditing ? (
                      <PermitRuleEditCells
                        state={editRule}
                        onChange={setEditRule}
                        options={permitRuleOptions}
                      />
                    ) : (
                      <PermitRuleReadCells value={v} options={permitRuleOptions} />
                    )
                  ) : null}
                  {showSchedule ? (
                    <ReferenceValueTableCell className="text-xs text-text-muted">
                      <button
                        type="button"
                        className="min-h-9 rounded text-xs font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                        onClick={() => setPatternValueId(v.id)}
                      >
                        {t("schedulePattern.open")}
                      </button>
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
                          aria-label={t("editor.colParent")}
                          className={cn(mfInput.nativeSelect, "h-9")}
                        >
                          <option value="">{t("editor.none")}</option>
                          {parentCandidates
                            .filter((p) => p.id !== v.id)
                            .map((p) => (
                              <option key={p.id} value={p.id}>
                                {parentLabel(p)}
                              </option>
                            ))}
                        </select>
                      ) : parentValue ? (
                        parentLabel(parentValue)
                      ) : (
                        "—"
                      )}
                    </ReferenceValueTableCell>
                  ) : null}
                  {showNature ? (
                    <ReferenceValueTableCell>
                      {isEditing ? (
                        <MaintenanceNatureSelect
                          value={editRow.natureTag}
                          natures={natures}
                          onChange={(natureTag) => setEditRow({ ...editRow, natureTag })}
                        />
                      ) : (
                        <MaintenanceNatureText value={v.semantic_tag ?? null} natures={natures} />
                      )}
                    </ReferenceValueTableCell>
                  ) : null}
                  {showAbsenceBehaviour ? (
                    <ReferenceValueTableCell>
                      {isEditing ? (
                        <>
                          <AbsenceBehaviourSelect
                            value={editRow.natureTag}
                            onChange={(natureTag) => setEditRow({ ...editRow, natureTag })}
                          />
                          {removesCriticalTag(v.semantic_tag, editRow.natureTag) ? (
                            <CriticalTagRemovalWarning />
                          ) : null}
                        </>
                      ) : (
                        <AbsenceBehaviourText value={v.semantic_tag ?? null} />
                      )}
                    </ReferenceValueTableCell>
                  ) : null}
                  {showColorColumn ? (
                    <ReferenceValueTableCell>
                      <ReferenceColorSwatchHex color={v.color_hex} />
                    </ReferenceValueTableCell>
                  ) : null}
                  <ReferenceValueTableCell>
                    <div className="flex items-center gap-2" title={switchReason}>
                      <Switch
                        checked={v.is_active}
                        onCheckedChange={(next) => void setValueActive(v.id, next)}
                        disabled={savingValue || !canDeactivateValue || isSystemRow}
                        aria-label={t("editor.activeToggle", { label: v.label })}
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
                          type="button"
                          variant="ghost"
                          size="icon"
                          className={refTableIconButtonClass()}
                          aria-label={t("editor.save")}
                          title={t("editor.save")}
                          onClick={() => void commitEdit(v.id)}
                          disabled={
                            savingValue ||
                            (showUnit && !unitEditComplete(editUnit)) ||
                            (showPermitRule && !permitRuleComplete(editRule))
                          }
                        >
                          <Check className="h-4 w-4 text-status-success" aria-hidden />
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="icon"
                          className={refTableIconButtonClass()}
                          aria-label={t("editor.cancel")}
                          title={t("editor.cancel")}
                          onClick={cancelEdit}
                        >
                          <X className="h-4 w-4" aria-hidden />
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
                            aria-label={t("editor.editValue", { label: v.label })}
                            title={t("editor.edit")}
                            onClick={() => startEdit(v)}
                          >
                            <Pencil className="h-4 w-4" aria-hidden />
                          </Button>
                        ) : null}
                        {showTrash ? (
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon"
                            className={refTableIconButtonClass()}
                            aria-label={t("editor.deactivateValue", { label: v.label })}
                            title={t("editor.deactivate")}
                            onClick={() => setDeleteTarget(v)}
                          >
                            <Trash2 className="h-4 w-4 text-status-danger" aria-hidden />
                          </Button>
                        ) : null}
                      </div>
                    ) : (
                      <span
                        className="text-xs text-text-muted"
                        title={lockedRow ? t("editor.disabledReason.systemValue") : undefined}
                      >
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

      {canImport ? (
        <ReferenceImportWizard
          domainId={domainId}
          targetSetId={setId}
          open={importOpen}
          onOpenChange={setImportOpen}
          onComplete={() => void loadValues(setId)}
        />
      ) : null}
    </>
  );
}
