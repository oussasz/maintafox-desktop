/**
 * WoCreateForm.tsx
 *
 * Create / edit-draft form for work orders.
 * Two sections: Work Order Info + Planning.
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import { Loader2, Search, X } from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { getStoredRamsEquipmentId } from "@/pages/reliability/rams-equipment-context";
import { searchAssets } from "@/services/asset-search-service";
import { getAssetById } from "@/services/asset-service";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderPrioritiesCatalog } from "@/stores/work-order-priorities-catalog-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type {
  Asset,
  AssetSearchResult,
  WoCreateInput,
  WorkOrder,
  WorkOrderPriorityOption,
} from "@shared/ipc-types";

function assetToSearchResult(a: Asset): AssetSearchResult {
  return {
    id: a.id,
    sync_id: a.sync_id,
    asset_code: a.asset_code,
    asset_name: a.asset_name,
    class_code: a.class_code,
    class_name: a.class_name,
    family_code: a.family_code,
    family_name: a.family_name,
    criticality_code: a.criticality_code,
    status_code: a.status_code,
    org_node_id: a.org_node_id,
    org_node_name: a.org_node_name,
    parent_asset_id: null,
    parent_asset_code: null,
    parent_asset_name: null,
    primary_meter_name: null,
    primary_meter_reading: null,
    primary_meter_unit: null,
    primary_meter_last_read_at: null,
    external_id_count: 0,
    row_version: a.row_version,
  };
}

const TITLE_MAX = 200;

function priorityDisplayLabel(p: WorkOrderPriorityOption, lang: string): string {
  return lang.toLowerCase().startsWith("fr") ? p.label_fr : p.label;
}

function parseOptionalIdString(raw: string): number | null {
  const s = raw.trim();
  if (!s) return null;
  const n = Number(s);
  return Number.isFinite(n) ? n : null;
}

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoCreateFormProps {
  initial: WorkOrder | null;
  /** When set (e.g. from URL), pre-select this equipment for new WOs. */
  prefillEquipmentId?: number | null;
  onSubmitted: (wo: WorkOrder) => void;
  onCancel: () => void;
}

// ── Validation ────────────────────────────────────────────────────────────────

interface FormErrors {
  title?: string;
  typeCode?: string;
  urgencyId?: string;
}

function validate(
  fields: { title: string; typeCode: string; urgencyId: string; urgencyIdValid: boolean },
  opts: { requireType: boolean; requireUrgency: boolean },
  t: (key: string) => string,
): FormErrors {
  const errors: FormErrors = {};
  if (!fields.title.trim()) errors.title = t("form.validation.titleRequired");
  if (opts.requireType && !fields.typeCode.trim())
    errors.typeCode = t("form.validation.typeRequired");
  if (opts.requireUrgency) {
    const raw = fields.urgencyId.trim();
    if (!raw) errors.urgencyId = t("form.validation.urgencyRequired");
    else if (!fields.urgencyIdValid) errors.urgencyId = t("form.validation.urgencyInvalid");
  }
  return errors;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCreateForm({
  initial,
  prefillEquipmentId,
  onSubmitted,
  onCancel,
}: WoCreateFormProps) {
  const { t, i18n } = useTranslation("ot");
  const { info } = useSession();
  const saving = useWoStore((s) => s.saving);
  const storeError = useWoStore((s) => s.error);
  const submitNewWo = useWoStore((s) => s.submitNewWo);
  const updateDraft = useWoStore((s) => s.updateDraft);

  const [submitError, setSubmitError] = useState<string | null>(null);

  const isEdit = initial !== null;

  // ── Form state ────────────────────────────────────────────────────────

  const [title, setTitle] = useState(initial?.title ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const woTypesCatalog = useWorkOrderTypesCatalog((s) => s.types);
  const typesLoading = useWorkOrderTypesCatalog((s) => s.loading);
  const catalogError = useWorkOrderTypesCatalog((s) => s.error);
  const loadWoTypes = useWorkOrderTypesCatalog((s) => s.load);
  const woTypes = useMemo(() => woTypesCatalog.filter((item) => item.is_active), [woTypesCatalog]);

  const prioritiesCatalog = useWorkOrderPrioritiesCatalog((s) => s.priorities);
  const prioritiesLoading = useWorkOrderPrioritiesCatalog((s) => s.loading);
  const prioritiesError = useWorkOrderPrioritiesCatalog((s) => s.error);
  const loadPriorities = useWorkOrderPrioritiesCatalog((s) => s.load);
  const activePriorities = useMemo(
    () => prioritiesCatalog.filter((p) => p.is_active),
    [prioritiesCatalog],
  );

  const [typeCode, setTypeCode] = useState<string>(initial?.type_code ?? "");
  const [urgencyId, setUrgencyId] = useState<string>(initial?.urgency_id?.toString() ?? "");
  const [plannedStart, setPlannedStart] = useState(initial?.planned_start ?? "");
  const [plannedEnd, setPlannedEnd] = useState(initial?.planned_end ?? "");
  const [expectedDuration, setExpectedDuration] = useState(
    initial?.expected_duration_hours?.toString() ?? "",
  );
  const [notes, setNotes] = useState(initial?.notes ?? "");

  // Equipment combobox state
  const [selectedAsset, setSelectedAsset] = useState<AssetSearchResult | null>(null);
  const [assetQuery, setAssetQuery] = useState("");
  const [assetResults, setAssetResults] = useState<AssetSearchResult[]>([]);
  const [assetSearching, setAssetSearching] = useState(false);
  const [showAssetDropdown, setShowAssetDropdown] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** RAMS prefill runs once per form mount; do not re-apply after the user clears or picks another asset. */
  const ramsEquipmentPrefillConsumedRef = useRef(false);
  /** Edit-mode equipment hydrate once per `initial` WO. */
  const editEquipmentHydrateKeyRef = useRef<string | null>(null);

  // Validation
  const [touched, setTouched] = useState<Set<string>>(new Set());

  useEffect(() => {
    void loadWoTypes();
  }, [loadWoTypes]);

  useEffect(() => {
    void loadPriorities();
  }, [loadPriorities]);

  useEffect(() => {
    if (isEdit) return;
    if (woTypes.length === 0) return;
    const codes = new Set(woTypes.map((w) => w.code));
    if (typeCode && codes.has(typeCode)) return;
    const firstType = woTypes.at(0);
    setTypeCode(firstType?.code ?? "");
  }, [isEdit, woTypes, typeCode]);

  useEffect(() => {
    if (isEdit) return;
    if (activePriorities.length === 0) return;
    const ids = new Set(activePriorities.map((p) => String(p.id)));
    if (urgencyId && ids.has(urgencyId)) return;
    const firstPri = activePriorities.at(0);
    if (!firstPri) return;
    setUrgencyId(String(firstPri.id));
  }, [isEdit, activePriorities, urgencyId]);

  // ── Equipment pre-fill for edit mode (once per opened WO; no loop after user clears) ──

  useEffect(() => {
    if (!initial?.equipment_id) return;
    const key = `${initial.id}-${initial.equipment_id}`;
    if (editEquipmentHydrateKeyRef.current === key) return;
    editEquipmentHydrateKeyRef.current = key;
    let cancelled = false;
    void searchAssets({ query: initial.asset_code ?? null, limit: 20 }).then((results) => {
      if (cancelled) return;
      const match = results.find((a) => a.id === initial.equipment_id);
      if (match) setSelectedAsset(match);
    });
    return () => {
      cancelled = true;
    };
  }, [initial?.id, initial?.equipment_id, initial?.asset_code]);

  // ── New WO: one-shot pre-fill from RAMS (localStorage) or prop ─────────────

  useEffect(() => {
    if (isEdit) return;
    if (ramsEquipmentPrefillConsumedRef.current) return;
    const hint = prefillEquipmentId ?? getStoredRamsEquipmentId();
    if (hint == null || hint <= 0) {
      ramsEquipmentPrefillConsumedRef.current = true;
      return;
    }
    ramsEquipmentPrefillConsumedRef.current = true;
    let cancelled = false;
    void getAssetById(hint)
      .then((asset) => {
        if (!cancelled) setSelectedAsset(assetToSearchResult(asset));
      })
      .catch(() => {
        /* Missing asset or IPC error — user can search manually */
      });
    return () => {
      cancelled = true;
    };
  }, [isEdit, prefillEquipmentId]);

  // ── Equipment search with debounce ────────────────────────────────────

  const handleAssetSearch = useCallback((query: string) => {
    setAssetQuery(query);
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current);
    if (query.length < 2) {
      setAssetResults([]);
      setShowAssetDropdown(false);
      return;
    }
    searchTimerRef.current = setTimeout(async () => {
      setAssetSearching(true);
      try {
        const results = await searchAssets({
          query,
          limit: 20,
          includeDecommissioned: false,
        });
        setAssetResults(results);
        setShowAssetDropdown(true);
      } finally {
        setAssetSearching(false);
      }
    }, 300);
  }, []);

  const handleSelectAsset = useCallback((asset: AssetSearchResult) => {
    setSelectedAsset(asset);
    setAssetQuery("");
    setAssetResults([]);
    setShowAssetDropdown(false);
  }, []);

  const handleClearAsset = useCallback(() => {
    setSelectedAsset(null);
    setAssetQuery("");
  }, []);

  // Close dropdown on outside click
  useEffect(() => {
    function handleClick(e: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowAssetDropdown(false);
      }
    }
    document.addEventListener("mousedown", handleClick);
    return () => document.removeEventListener("mousedown", handleClick);
  }, []);

  // ── Validation ────────────────────────────────────────────────────────

  /** Create flow always requires an explicit type; edit only when the catalog has rows. */
  const requireType = !isEdit || woTypes.length > 0;
  const requireUrgency = activePriorities.length > 0;
  const urgencyIdValid =
    !requireUrgency || activePriorities.some((p) => String(p.id) === urgencyId.trim());

  const currentErrors = useMemo(
    () =>
      validate({ title, typeCode, urgencyId, urgencyIdValid }, { requireType, requireUrgency }, t),
    [title, typeCode, urgencyId, urgencyIdValid, requireType, requireUrgency, t],
  );
  const isValid = Object.keys(currentErrors).length === 0;
  const catalogsReady = !typesLoading && !prioritiesLoading;

  const markTouched = useCallback((field: string) => {
    setTouched((prev) => new Set(prev).add(field));
  }, []);

  const fieldError = useCallback(
    (field: keyof FormErrors) => (touched.has(field) ? currentErrors[field] : undefined),
    [touched, currentErrors],
  );

  // ── Submit ────────────────────────────────────────────────────────────

  const handleSubmit = useCallback(async () => {
    setTouched(new Set(["title", "typeCode", "urgencyId"]));
    setSubmitError(null);
    if (!isValid || !catalogsReady || !info?.user_id) return;

    const resolvedEquipmentId =
      selectedAsset != null && selectedAsset.id > 0 ? selectedAsset.id : null;

    try {
      if (isEdit && initial) {
        const draftPayload = {
          id: initial.id,
          expected_row_version: initial.row_version,
          title: title.trim(),
          description: description.trim() || null,
          type_code: typeCode || null,
          equipment_id: resolvedEquipmentId,
          urgency_id: parseOptionalIdString(urgencyId),
          planned_start: plannedStart || null,
          planned_end: plannedEnd || null,
          expected_duration_hours: expectedDuration ? Number(expectedDuration) : null,
          notes: notes.trim() || null,
        };
        // eslint-disable-next-line no-console -- debug: OT payload to IPC
        console.log("OT Payload:", draftPayload);
        await updateDraft(draftPayload);
        onSubmitted({ ...initial, title, description });
      } else {
        const input: WoCreateInput = {
          title: title.trim(),
          description: description.trim() || null,
          type_code: typeCode.trim(),
          equipment_id: resolvedEquipmentId,
          urgency_id: parseOptionalIdString(urgencyId),
          planned_start: plannedStart || null,
          planned_end: plannedEnd || null,
          expected_duration_hours: expectedDuration ? Number(expectedDuration) : null,
          notes: notes.trim() || null,
          creator_id: info.user_id,
        };
        // eslint-disable-next-line no-console -- debug: OT payload to IPC
        console.log("OT Payload:", input);
        const wo = await submitNewWo(input);
        onSubmitted(wo);
      }
    } catch (err) {
      setSubmitError(toErrorMessage(err));
    }
  }, [
    isValid,
    catalogsReady,
    info,
    isEdit,
    initial,
    title,
    description,
    typeCode,
    selectedAsset,
    urgencyId,
    plannedStart,
    plannedEnd,
    expectedDuration,
    notes,
    submitNewWo,
    updateDraft,
    onSubmitted,
  ]);

  // ── Render ────────────────────────────────────────────────────────────

  return (
    <div className="flex flex-col gap-6 overflow-y-auto max-h-[70vh] px-1 py-2">
      {/* ── Section 1: Work Order Info ───────────────────────────────── */}
      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-text-primary">{t("detail.sections.general")}</h3>

        {/* Type */}
        <FormField
          name="type"
          label={t("form.type.label")}
          error={fieldError("typeCode")}
          required={requireType}
        >
          <Select
            {...(typeCode ? { value: typeCode } : {})}
            onValueChange={(v) => {
              setTypeCode(v);
              markTouched("typeCode");
            }}
          >
            <SelectTrigger className="w-full">
              <SelectValue placeholder={typesLoading ? "Chargement..." : t("form.type.label")} />
            </SelectTrigger>
            <SelectContent>
              {woTypes.map((wt) => (
                <SelectItem key={wt.id} value={wt.code}>
                  {t(`type.${wt.code === "condition_based" ? "conditionBased" : wt.code}`, {
                    defaultValue: wt.label,
                  })}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        {/* Equipment */}
        <FormField name="equipment" label={t("form.equipment.label")}>
          {selectedAsset ? (
            <div className="rounded-lg border border-surface-border bg-surface-1 p-3">
              <div className="flex items-start justify-between">
                <div className="space-y-0.5">
                  <p className="text-sm font-medium text-text-primary">
                    {selectedAsset.asset_code} — {selectedAsset.asset_name}
                  </p>
                  {selectedAsset.family_name && (
                    <p className="text-xs text-text-muted">{selectedAsset.family_name}</p>
                  )}
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 w-6 p-0"
                  onClick={handleClearAsset}
                >
                  <X className="h-3.5 w-3.5" />
                </Button>
              </div>
            </div>
          ) : (
            <div ref={dropdownRef} className="relative">
              <div className="relative">
                <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
                <Input
                  id="wo-equipment-search"
                  className="pl-9"
                  placeholder={t("form.equipment.placeholder")}
                  value={assetQuery}
                  onChange={(e: ChangeEvent<HTMLInputElement>) => handleAssetSearch(e.target.value)}
                  onFocus={() => {
                    if (assetResults.length > 0) setShowAssetDropdown(true);
                  }}
                />
                {assetSearching && (
                  <Loader2 className="absolute right-2.5 top-2.5 h-4 w-4 animate-spin text-text-muted" />
                )}
              </div>

              {showAssetDropdown && assetResults.length > 0 && (
                <div className="absolute z-50 mt-1 w-full rounded-md border border-surface-border bg-surface-0 shadow-lg max-h-60 overflow-y-auto">
                  {assetResults.map((asset) => (
                    <button
                      key={asset.id}
                      type="button"
                      className="flex w-full items-start gap-2 px-3 py-2 text-left text-sm hover:bg-surface-1 transition-colors"
                      onMouseDown={(e) => {
                        e.preventDefault();
                        handleSelectAsset(asset);
                      }}
                    >
                      <span className="font-mono text-xs text-text-muted shrink-0">
                        {asset.asset_code}
                      </span>
                      <span className="truncate">{asset.asset_name}</span>
                      {asset.family_name && (
                        <Badge variant="outline" className="text-[10px] ml-auto shrink-0">
                          {asset.family_name}
                        </Badge>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </div>
          )}
        </FormField>

        {/* Urgency */}
        <FormField
          name="urgency"
          label={t("form.urgency.label")}
          error={fieldError("urgencyId")}
          required={requireUrgency}
        >
          <Select
            {...(urgencyId ? { value: urgencyId } : {})}
            onValueChange={(v) => {
              setUrgencyId(v);
              markTouched("urgencyId");
            }}
          >
            <SelectTrigger className="w-full">
              <SelectValue placeholder={prioritiesLoading ? "…" : t("form.urgency.label")} />
            </SelectTrigger>
            <SelectContent>
              {activePriorities.map((p) => (
                <SelectItem key={p.id} value={String(p.id)}>
                  <span style={{ color: p.hex_color }}>
                    {priorityDisplayLabel(p, i18n.language)}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        {/* Title */}
        <FormField name="title" label={t("form.title.label")} error={fieldError("title")} required>
          <div className="relative">
            <Input
              id="wo-title"
              value={title}
              maxLength={TITLE_MAX}
              placeholder={t("form.title.placeholder")}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setTitle(e.target.value)}
              onBlur={() => markTouched("title")}
            />
            <span className="absolute right-2 top-2 text-[10px] text-text-muted">
              {title.length}/{TITLE_MAX}
            </span>
          </div>
        </FormField>

        {/* Description */}
        <FormField name="description" label={t("form.description.label")}>
          <Textarea
            id="wo-description"
            value={description}
            placeholder={t("form.description.placeholder")}
            rows={3}
            onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setDescription(e.target.value)}
          />
        </FormField>
      </section>

      <Separator />

      {/* ── Section 2: Planning ──────────────────────────────────────── */}
      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-text-primary">{t("detail.sections.planning")}</h3>

        <div className="grid grid-cols-2 gap-4">
          {/* Planned Start */}
          <FormField name="plannedStart" label={t("form.plannedStart.label")}>
            <Input
              id="wo-planned-start"
              type="datetime-local"
              value={plannedStart}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setPlannedStart(e.target.value)}
            />
          </FormField>

          {/* Planned End */}
          <FormField name="plannedEnd" label={t("form.plannedEnd.label")}>
            <Input
              id="wo-planned-end"
              type="datetime-local"
              value={plannedEnd}
              min={plannedStart || undefined}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setPlannedEnd(e.target.value)}
            />
          </FormField>
        </div>

        {/* Expected Duration */}
        <FormField name="duration" label={t("form.duration.label")}>
          <Input
            id="wo-duration"
            type="number"
            min={0}
            step={0.5}
            value={expectedDuration}
            onChange={(e: ChangeEvent<HTMLInputElement>) => setExpectedDuration(e.target.value)}
          />
        </FormField>

        {/* Notes */}
        <FormField name="notes" label={t("form.notes.label")}>
          <Textarea
            id="wo-notes"
            value={notes}
            rows={2}
            onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setNotes(e.target.value)}
          />
        </FormField>
      </section>

      <Separator />

      {/* ── Error banner ────────────────────────────────────────────── */}
      {(submitError || storeError || catalogError || prioritiesError) && (
        <div className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
          {[submitError, storeError, catalogError, prioritiesError]
            .filter(Boolean)
            .map((e) => toErrorMessage(e))
            .join(" ")}
        </div>
      )}

      {/* ── Actions ──────────────────────────────────────────────────── */}
      <div className="flex justify-end gap-2">
        <Button variant="outline" size="sm" onClick={onCancel} disabled={saving}>
          {t("form.cancel")}
        </Button>
        <Button
          size="sm"
          onClick={() => void handleSubmit()}
          disabled={saving || !isValid || !catalogsReady}
        >
          {saving && <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />}
          {isEdit ? t("form.update") : t("form.submit")}
        </Button>
      </div>
    </div>
  );
}
