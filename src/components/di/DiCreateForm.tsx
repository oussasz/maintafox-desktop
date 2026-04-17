/**
 * DiCreateForm.tsx
 *
 * Multi-section create / edit form for Intervention Requests (DI).
 * Three visual sections: Requester (read-only), Equipment Selection,
 * General Information. Client-side validation with inline errors.
 *
 * Phase 2 – Sub-phase 04 – File 01 – Sprint S4.
 */

import { Loader2, Search, X } from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { useDiStore } from "@/stores/di-store";
import type {
  AssetSearchResult,
  DiCreateInput,
  DiImpactLevel,
  DiOriginType,
  DiUrgency,
  InterventionRequest,
} from "@shared/ipc-types";

// ── Constants ─────────────────────────────────────────────────────────────────

const ORIGIN_TYPES: DiOriginType[] = [
  "operator",
  "technician",
  "inspection",
  "pm",
  "iot",
  "quality",
  "hse",
  "production",
  "external",
];

const URGENCY_LEVELS: DiUrgency[] = ["low", "medium", "high", "critical"];

const IMPACT_LEVELS: DiImpactLevel[] = ["unknown", "none", "minor", "major", "critical"];

const TITLE_MAX = 100;
const DESC_MAX = 1000;

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiCreateFormProps {
  initial: InterventionRequest | null;
  onSubmitted: (di: InterventionRequest) => void;
  onCancel: () => void;
}

// ── Validation ────────────────────────────────────────────────────────────────

interface FormErrors {
  title?: string;
  description?: string;
  origin_type?: string;
  reported_urgency?: string;
  impact_level?: string;
  equipment?: string;
}

function validate(
  fields: {
    title: string;
    description: string;
    origin_type: string;
    reported_urgency: string;
    impact_level: string;
    assetId: number | null;
  },
  t: (key: string) => string,
): FormErrors {
  const errors: FormErrors = {};
  if (!fields.title.trim()) errors.title = t("form.validation.titleRequired");
  if (!fields.description.trim()) errors.description = t("form.validation.descriptionRequired");
  if (!fields.origin_type) errors.origin_type = t("form.validation.originRequired");
  if (!fields.reported_urgency) errors.reported_urgency = t("form.validation.urgencyRequired");
  if (!fields.impact_level) errors.impact_level = t("form.validation.impactRequired");
  if (!fields.assetId) errors.equipment = t("form.validation.equipmentRequired");
  return errors;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiCreateForm({ initial, onSubmitted, onCancel }: DiCreateFormProps) {
  const { t } = useTranslation("di");
  const { info } = useSession();
  const saving = useDiStore((s) => s.saving);
  const storeError = useDiStore((s) => s.error);
  const submitNewDi = useDiStore((s) => s.submitNewDi);
  const updateDraft = useDiStore((s) => s.updateDraft);

  const [submitError, setSubmitError] = useState<string | null>(null);

  const isEdit = initial !== null;

  // ── Form state ────────────────────────────────────────────────────────

  const [title, setTitle] = useState(initial?.title ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [originType, setOriginType] = useState<string>(initial?.origin_type ?? "");
  const [urgency, setUrgency] = useState<string>(initial?.reported_urgency ?? "");
  const [impactLevel, setImpactLevel] = useState<string>(initial?.impact_level ?? "");
  const [observedAt, setObservedAt] = useState(initial?.observed_at ?? "");
  const [safetyFlag, setSafetyFlag] = useState(initial?.safety_flag ?? false);
  const [environmentalFlag, setEnvironmentalFlag] = useState(initial?.environmental_flag ?? false);
  const [qualityFlag, setQualityFlag] = useState(initial?.quality_flag ?? false);
  const [productionImpact, setProductionImpact] = useState(initial?.production_impact ?? false);

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
  const [, setErrors] = useState<FormErrors>({});

  // ── Equipment pre-fill for edit mode ──────────────────────────────────

  useEffect(() => {
    if (initial?.asset_id && !selectedAsset) {
      void searchAssets({ query: null, limit: 1 }).then((results) => {
        const match = results.find((a) => a.id === initial.asset_id);
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

  const currentErrors = useMemo(
    () =>
      validate(
        {
          title,
          description,
          origin_type: originType,
          reported_urgency: urgency,
          impact_level: impactLevel,
          assetId: selectedAsset?.id ?? null,
        },
        (key) => t(key as never),
      ),
    [title, description, originType, urgency, impactLevel, selectedAsset, t],
  );

  const isValid = Object.keys(currentErrors).length === 0;

  const markTouched = useCallback((field: string) => {
    setTouched((prev) => new Set(prev).add(field));
  }, []);

  // Show error only if touched
  const fieldError = useCallback(
    (field: keyof FormErrors) => (touched.has(field) ? currentErrors[field] : undefined),
    [touched, currentErrors],
  );

  // ── Submit ────────────────────────────────────────────────────────────

  const handleSubmit = useCallback(async () => {
    // Mark all fields as touched to show all errors
    setTouched(
      new Set([
        "title",
        "description",
        "origin_type",
        "reported_urgency",
        "impact_level",
        "equipment",
      ]),
    );
    setErrors(currentErrors);
    setSubmitError(null);

    if (!isValid || !selectedAsset || !info?.user_id) return;

    if (selectedAsset.org_node_id == null) {
      setSubmitError(t("form.validation.assetMissingOrgNode" as never));
      return;
    }

    try {
      if (isEdit && initial) {
        await updateDraft({
          id: initial.id,
          expected_row_version: initial.row_version,
          title,
          description,
          impact_level: impactLevel,
          production_impact: productionImpact,
          safety_flag: safetyFlag,
          environmental_flag: environmentalFlag,
          quality_flag: qualityFlag,
          reported_urgency: urgency,
          observed_at: observedAt || null,
        });
        // Return updated DI (reload from store after updateDraft)
        onSubmitted({ ...initial, title, description });
      } else {
        const input: DiCreateInput = {
          asset_id: selectedAsset.id,
          org_node_id: selectedAsset.org_node_id,
          title: title.trim(),
          description: description.trim(),
          origin_type: originType,
          impact_level: impactLevel,
          production_impact: productionImpact,
          safety_flag: safetyFlag,
          environmental_flag: environmentalFlag,
          quality_flag: qualityFlag,
          reported_urgency: urgency,
          observed_at: observedAt || null,
          submitter_id: info.user_id,
        };
        const di = await submitNewDi(input);
        onSubmitted(di);
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setSubmitError(msg);
    }
  }, [
    currentErrors,
    isValid,
    selectedAsset,
    info,
    isEdit,
    initial,
    title,
    description,
    originType,
    urgency,
    impactLevel,
    observedAt,
    safetyFlag,
    environmentalFlag,
    qualityFlag,
    productionImpact,
    submitNewDi,
    updateDraft,
    onSubmitted,
    t,
  ]);

  // ── Render ────────────────────────────────────────────────────────────

  return (
    <div className="flex flex-col gap-6 overflow-y-auto max-h-[70vh] px-1 py-2">
      {/* ── Section 1: Requester (read-only) ─────────────────────────── */}
      <section>
        <h3 className="text-sm font-semibold text-text-primary mb-3">
          {t("form.section.requester")}
        </h3>
        <div className="rounded-lg border border-surface-border bg-surface-1 p-3 space-y-1">
          <p className="text-sm text-text-primary">{info?.display_name ?? "—"}</p>
          <p className="text-xs text-text-muted">{info?.username ?? ""}</p>
        </div>
      </section>

      <Separator />

      {/* ── Section 2: Equipment Selection ───────────────────────────── */}
      <section>
        <h3 className="text-sm font-semibold text-text-primary mb-3">
          {t("form.section.equipment")}
        </h3>

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
                {selectedAsset.org_node_name && (
                  <p className="text-xs text-text-muted">{selectedAsset.org_node_name}</p>
                )}
              </div>
              <Button variant="ghost" size="sm" className="h-6 w-6 p-0" onClick={handleClearAsset}>
                <X className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>
        ) : (
          <div ref={dropdownRef} className="relative">
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
              <Input
                id="equipment-search"
                className="pl-9"
                placeholder={t("form.equipmentSearch")}
                value={assetQuery}
                onChange={(e: ChangeEvent<HTMLInputElement>) => handleAssetSearch(e.target.value)}
                onFocus={() => {
                  if (assetResults.length > 0) setShowAssetDropdown(true);
                }}
                onBlur={() => markTouched("equipment")}
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

            {fieldError("equipment") && (
              <p className="text-xs text-status-danger mt-1">{fieldError("equipment")}</p>
            )}
          </div>
        )}
      </section>

      <Separator />

      {/* ── Section 3: General Information ───────────────────────────── */}
      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-text-primary">{t("form.section.general")}</h3>

        {/* Title */}
        <FormField
          name="title"
          label={t("form.subject.label")}
          error={fieldError("title")}
          required
        >
          <div className="relative">
            <Input
              id="title"
              value={title}
              maxLength={TITLE_MAX}
              onChange={(e: ChangeEvent<HTMLInputElement>) => setTitle(e.target.value)}
              onBlur={() => markTouched("title")}
              placeholder={t("form.subject.placeholder")}
            />
            <span className="absolute right-2 top-2 text-[10px] text-text-muted">
              {title.length}/{TITLE_MAX}
            </span>
          </div>
        </FormField>

        {/* Origin type */}
        <FormField
          name="origin_type"
          label={t("form.originType.label")}
          error={fieldError("origin_type")}
          required
        >
          <Select
            value={originType}
            onValueChange={(v) => {
              setOriginType(v);
              markTouched("origin_type");
            }}
          >
            <SelectTrigger id="origin_type">
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              {ORIGIN_TYPES.map((o) => (
                <SelectItem key={o} value={o}>
                  {t(`form.origin.${o}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        {/* Urgency */}
        <FormField
          name="reported_urgency"
          label={t("form.priority.label")}
          error={fieldError("reported_urgency")}
          required
        >
          <Select
            value={urgency}
            onValueChange={(v) => {
              setUrgency(v);
              markTouched("reported_urgency");
            }}
          >
            <SelectTrigger id="reported_urgency">
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              {URGENCY_LEVELS.map((u) => (
                <SelectItem key={u} value={u}>
                  {t(`priority.${u}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        {/* Impact level */}
        <FormField
          name="impact_level"
          label={t("form.impactLevel.label")}
          error={fieldError("impact_level")}
          required
        >
          <Select
            value={impactLevel}
            onValueChange={(v) => {
              setImpactLevel(v);
              markTouched("impact_level");
            }}
          >
            <SelectTrigger id="impact_level">
              <SelectValue placeholder="—" />
            </SelectTrigger>
            <SelectContent>
              {IMPACT_LEVELS.map((l) => (
                <SelectItem key={l} value={l}>
                  {t(`form.impact.${l}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>

        {/* Description */}
        <FormField
          name="description"
          label={t("form.description.label")}
          error={fieldError("description")}
          required
        >
          <div className="relative">
            <Textarea
              id="description"
              value={description}
              maxLength={DESC_MAX}
              rows={4}
              onChange={(e: ChangeEvent<HTMLTextAreaElement>) => setDescription(e.target.value)}
              onBlur={() => markTouched("description")}
              placeholder={t("form.description.placeholder")}
            />
            <span className="absolute right-2 bottom-2 text-[10px] text-text-muted">
              {description.length}/{DESC_MAX}
            </span>
          </div>
        </FormField>

        {/* Observed at */}
        <FormField name="observed_at" label={t("form.observedAt.label")}>
          <Input
            id="observed_at"
            type="datetime-local"
            value={observedAt}
            onChange={(e: ChangeEvent<HTMLInputElement>) => setObservedAt(e.target.value)}
          />
        </FormField>

        {/* Flag checkboxes */}
        <div className="grid grid-cols-2 gap-3">
          <label htmlFor="safety_flag" className="flex items-center gap-2 text-sm">
            <Checkbox
              id="safety_flag"
              checked={safetyFlag}
              onCheckedChange={(v) => setSafetyFlag(v === true)}
            />
            {t("form.safetyFlag.label")}
          </label>
          <label htmlFor="environmental_flag" className="flex items-center gap-2 text-sm">
            <Checkbox
              id="environmental_flag"
              checked={environmentalFlag}
              onCheckedChange={(v) => setEnvironmentalFlag(v === true)}
            />
            {t("form.environmentalFlag.label")}
          </label>
          <label htmlFor="quality_flag" className="flex items-center gap-2 text-sm">
            <Checkbox
              id="quality_flag"
              checked={qualityFlag}
              onCheckedChange={(v) => setQualityFlag(v === true)}
            />
            {t("form.qualityFlag.label")}
          </label>
          <label htmlFor="production_impact" className="flex items-center gap-2 text-sm">
            <Checkbox
              id="production_impact"
              checked={productionImpact}
              onCheckedChange={(v) => setProductionImpact(v === true)}
            />
            {t("form.productionImpact.label")}
          </label>
        </div>
      </section>

      {/* ── Error banner ────────────────────────────────────────────── */}
      {(submitError || storeError) && (
        <div className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
          {submitError || storeError}
        </div>
      )}

      {/* ── Footer ───────────────────────────────────────────────────── */}
      <div className="flex items-center justify-end gap-2 pt-2 border-t border-surface-border">
        <Button variant="outline" onClick={onCancel} disabled={saving}>
          {t("form.cancel")}
        </Button>
        <Button
          onClick={() => void handleSubmit()}
          disabled={saving || (!isValid && touched.size > 0)}
        >
          {saving && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
          {isEdit ? t("form.update") : t("form.submit")}
        </Button>
      </div>
    </div>
  );
}
