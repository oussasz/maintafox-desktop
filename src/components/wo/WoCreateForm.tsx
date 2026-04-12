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
import { searchAssets } from "@/services/asset-search-service";
import { useWoStore } from "@/stores/wo-store";
import type { AssetSearchResult, WoCreateInput, WorkOrder } from "@shared/ipc-types";

// ── Constants ─────────────────────────────────────────────────────────────────

const WO_TYPES = [
  { id: 1, key: "corrective" },
  { id: 2, key: "preventive" },
  { id: 3, key: "predictive" },
  { id: 4, key: "improvement" },
  { id: 5, key: "inspection" },
  { id: 6, key: "overhaul" },
  { id: 7, key: "conditionBased" },
] as const;

const URGENCY_LEVELS = [
  { id: 1, key: "veryLow" },
  { id: 2, key: "low" },
  { id: 3, key: "medium" },
  { id: 4, key: "high" },
  { id: 5, key: "critical" },
] as const;

const URGENCY_COLOR: Record<number, string> = {
  1: "text-gray-500",
  2: "text-green-600",
  3: "text-yellow-600",
  4: "text-orange-600",
  5: "text-red-600",
};

const TITLE_MAX = 200;

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoCreateFormProps {
  initial: WorkOrder | null;
  onSubmitted: (wo: WorkOrder) => void;
  onCancel: () => void;
}

// ── Validation ────────────────────────────────────────────────────────────────

interface FormErrors {
  title?: string;
}

function validate(fields: { title: string }, t: (key: string) => string): FormErrors {
  const errors: FormErrors = {};
  if (!fields.title.trim()) errors.title = t("form.validation.titleRequired");
  return errors;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCreateForm({ initial, onSubmitted, onCancel }: WoCreateFormProps) {
  const { t } = useTranslation("ot");
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
  const [typeId, setTypeId] = useState<string>(initial?.type_id?.toString() ?? "");
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

  // Validation
  const [touched, setTouched] = useState<Set<string>>(new Set());

  // ── Equipment pre-fill for edit mode ──────────────────────────────────

  useEffect(() => {
    if (initial?.equipment_id && !selectedAsset) {
      void searchAssets({ query: initial.asset_code ?? null, limit: 20 }).then((results) => {
        const match = results.find((a) => a.id === initial.equipment_id);
        if (match) setSelectedAsset(match);
      });
    }
  }, [initial, selectedAsset]);

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

  const currentErrors = useMemo(() => validate({ title }, t), [title, t]);
  const isValid = Object.keys(currentErrors).length === 0;

  const markTouched = useCallback((field: string) => {
    setTouched((prev) => new Set(prev).add(field));
  }, []);

  const fieldError = useCallback(
    (field: keyof FormErrors) => (touched.has(field) ? currentErrors[field] : undefined),
    [touched, currentErrors],
  );

  // ── Submit ────────────────────────────────────────────────────────────

  const handleSubmit = useCallback(async () => {
    setTouched(new Set(["title"]));
    setSubmitError(null);
    if (!isValid || !info?.user_id) return;

    try {
      if (isEdit && initial) {
        await updateDraft({
          id: initial.id,
          expected_row_version: initial.row_version,
          title: title.trim(),
          description: description.trim() || null,
          type_id: typeId ? Number(typeId) : null,
          equipment_id: selectedAsset?.id ?? null,
          urgency_id: urgencyId ? Number(urgencyId) : null,
          planned_start: plannedStart || null,
          planned_end: plannedEnd || null,
          expected_duration_hours: expectedDuration ? Number(expectedDuration) : null,
          notes: notes.trim() || null,
        });
        onSubmitted({ ...initial, title, description });
      } else {
        const input: WoCreateInput = {
          title: title.trim(),
          description: description.trim() || null,
          type_id: typeId ? Number(typeId) : 1,
          equipment_id: selectedAsset?.id ?? null,
          urgency_id: urgencyId ? Number(urgencyId) : null,
          planned_start: plannedStart || null,
          planned_end: plannedEnd || null,
          expected_duration_hours: expectedDuration ? Number(expectedDuration) : null,
          notes: notes.trim() || null,
          creator_id: info.user_id,
        };
        const wo = await submitNewWo(input);
        onSubmitted(wo);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setSubmitError(msg);
    }
  }, [
    isValid,
    info,
    isEdit,
    initial,
    title,
    description,
    typeId,
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
        <FormField name="type" label={t("form.type.label")}>
          <Select value={typeId} onValueChange={setTypeId}>
            <SelectTrigger className="w-full">
              <SelectValue placeholder={t("form.type.label")} />
            </SelectTrigger>
            <SelectContent>
              {WO_TYPES.map((wt) => (
                <SelectItem key={wt.id} value={wt.id.toString()}>
                  {t(`type.${wt.key}`)}
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
        <FormField name="urgency" label={t("form.urgency.label")}>
          <Select value={urgencyId} onValueChange={setUrgencyId}>
            <SelectTrigger className="w-full">
              <SelectValue placeholder={t("form.urgency.label")} />
            </SelectTrigger>
            <SelectContent>
              {URGENCY_LEVELS.map((u) => (
                <SelectItem key={u.id} value={u.id.toString()}>
                  <span className={URGENCY_COLOR[u.id]}>{t(`form.urgency.${u.key}`)}</span>
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
      {(submitError || storeError) && (
        <div className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
          {submitError || storeError}
        </div>
      )}

      {/* ── Actions ──────────────────────────────────────────────────── */}
      <div className="flex justify-end gap-2">
        <Button variant="outline" size="sm" onClick={onCancel} disabled={saving}>
          {t("form.cancel")}
        </Button>
        <Button size="sm" onClick={() => void handleSubmit()} disabled={saving || !isValid}>
          {saving && <Loader2 className="h-3.5 w-3.5 animate-spin mr-1.5" />}
          {isEdit ? t("form.update") : t("form.submit")}
        </Button>
      </div>
    </div>
  );
}
