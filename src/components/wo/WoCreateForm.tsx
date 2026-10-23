/**
 * WoCreateForm.tsx
 *
 * Create / edit-draft form for work orders.
 * Two sections: Work Order Info + Planning.
 * Phase 2 – Sub-phase 05 – File 01 – Sprint S4.
 */

import { Loader2 } from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { UrgencySelect } from "@/components/common/urgency/UrgencySelect";
import { fromDateTimeLocal, toDateTimeLocal } from "@/components/planning/planning-model";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { DateTimeField } from "@/components/ui/date-time-field";
import { DecimalField } from "@/components/ui/decimal-field";
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
import { describeAppError } from "@/lib/app-error";
import { assetToSearchResult } from "@/lib/asset-to-search-result";
import { getAssetByIdSilent } from "@/services/asset-service";
import { listWorkOrderSubtypes } from "@/services/wo-service";
import { useUrgencyScaleStore } from "@/stores/urgency-scale-store";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import type {
  AssetSearchResult,
  UrgencyCode,
  WoCreateInput,
  WorkOrder,
  WorkOrderSubtypeOption,
} from "@shared/ipc-types";

import { WoPermitRequirementField } from "./WoPermitRequirementField";
import { permitRuleFlagActive } from "./WoPermitRuleNote";
import { woNatureLabel, woTypeLabel } from "./use-wo-type-labels";

const TITLE_MAX = 200;
/** Select value for "no sub-type" (Radix Select needs a non-empty value). */
const NO_SUBTYPE = "__none";

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoCreateFormProps {
  initial: WorkOrder | null;
  /** Snapshot prefill when opening create from asset context. */
  prefillAsset?: AssetSearchResult | null;
  /** When set (e.g. RAMS / URL), pre-select this equipment for new WOs. */
  prefillEquipmentId?: number | null;
  onSubmitted: (wo: WorkOrder) => void;
  onCancel: () => void;
}

// ── Validation ────────────────────────────────────────────────────────────────

interface FormErrors {
  title?: string;
  typeCode?: string;
  urgencyId?: string;
  equipment?: string;
}

function validate(
  fields: {
    title: string;
    typeCode: string;
    urgencyId: string;
    urgencyIdValid: boolean;
    equipmentId: number | null;
  },
  opts: { requireType: boolean; requireUrgency: boolean; requireEquipment: boolean },
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
  if (opts.requireEquipment && fields.equipmentId == null) {
    errors.equipment = t("form.validation.equipmentRequired");
  }
  return errors;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCreateForm({
  initial,
  prefillAsset = null,
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

  // Shared DI / OT urgency scale (levels fixed by Maintafox, labels / targets by the plant).
  const urgencyLevels = useUrgencyScaleStore((s) => s.levels);
  const prioritiesLoading = useUrgencyScaleStore((s) => s.loading);
  const prioritiesError = useUrgencyScaleStore((s) => s.error);

  const [typeCode, setTypeCode] = useState<string>(initial?.type_code ?? "");
  // The plant's sub-types (COMPANY list), each under one maintenance nature.
  const [subtypes, setSubtypes] = useState<WorkOrderSubtypeOption[]>([]);
  const [subtypeCode, setSubtypeCode] = useState<string>(initial?.subtype_code ?? "");
  const [urgencyId, setUrgencyId] = useState<string>(initial?.urgency_code ?? "");
  // The inputs hold local date and time (`datetime-local`); the work order stores UTC instants.
  const [plannedStart, setPlannedStart] = useState(
    initial?.planned_start ? toDateTimeLocal(initial.planned_start) : "",
  );
  const [plannedEnd, setPlannedEnd] = useState(
    initial?.planned_end ? toDateTimeLocal(initial.planned_end) : "",
  );
  const [expectedDuration, setExpectedDuration] = useState<number | null>(
    initial?.expected_duration_hours ?? null,
  );
  const [notes, setNotes] = useState(initial?.notes ?? "");
  const [requiresPermit, setRequiresPermit] = useState<boolean>(initial?.requires_permit ?? false);
  // A requirement set by a plant rule or carried from a request flagged "sécurité" is not an edit
  // of the work order (the backend refuses the untick): cleared from the planning panel with the
  // permit-issue right and a reason.
  const permitLocked = initial != null && permitRuleFlagActive(initial);
  const permitLockedReason = !permitLocked
    ? null
    : initial?.permit_rule_code != null
      ? t("permitRule.checkboxLocked", { rule: initial.permit_rule_code })
      : t("permitRule.clearHint");

  // Equipment selection (shared AssetPicker)
  const [selectedAsset, setSelectedAsset] = useState<AssetSearchResult | null>(null);
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
    let cancelled = false;
    listWorkOrderSubtypes()
      .then((rows) => {
        if (!cancelled) setSubtypes(rows.filter((r) => r.is_active));
      })
      .catch((err: unknown) => {
        if (!cancelled) setSubmitError(describeAppError(err).title);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  /** Sub-types of the selected type's nature (EN 13306); the rules keep reading the nature. */
  const selectedNature = woTypes.find((w) => w.code === typeCode)?.nature ?? null;
  const subtypeOptions = useMemo(
    () => (selectedNature ? subtypes.filter((st) => st.nature === selectedNature) : []),
    [subtypes, selectedNature],
  );
  useEffect(() => {
    if (subtypeCode && !subtypeOptions.some((st) => st.code === subtypeCode)) setSubtypeCode("");
  }, [subtypeCode, subtypeOptions]);

  useEffect(() => {
    if (isEdit) return;
    if (woTypes.length === 0) return;
    const codes = new Set(woTypes.map((w) => w.code));
    if (typeCode && codes.has(typeCode)) return;
    const firstType = woTypes.at(0);
    setTypeCode(firstType?.code ?? "");
  }, [isEdit, woTypes, typeCode]);

  // ── Equipment pre-fill for edit mode (once per opened WO; no loop after user clears) ──

  useEffect(() => {
    if (!initial?.equipment_id) return;
    const key = `${initial.id}-${initial.equipment_id}`;
    if (editEquipmentHydrateKeyRef.current === key) return;
    editEquipmentHydrateKeyRef.current = key;
    let cancelled = false;
    void getAssetByIdSilent(initial.equipment_id).then((asset) => {
      if (!cancelled && asset) setSelectedAsset(assetToSearchResult(asset));
    });
    return () => {
      cancelled = true;
    };
  }, [initial?.id, initial?.equipment_id]);

  // ── New WO: one-shot pre-fill only from what the caller passes ────────────
  // (an asset or an equipment id); never from a selection left over by another screen.

  useEffect(() => {
    if (isEdit) return;
    if (ramsEquipmentPrefillConsumedRef.current) return;

    if (prefillAsset) {
      ramsEquipmentPrefillConsumedRef.current = true;
      setSelectedAsset(prefillAsset);
      return;
    }

    const hint = prefillEquipmentId;
    if (hint == null || hint <= 0) {
      return;
    }

    ramsEquipmentPrefillConsumedRef.current = true;
    let cancelled = false;
    void getAssetByIdSilent(hint).then((asset) => {
      if (!cancelled && asset) setSelectedAsset(assetToSearchResult(asset));
    });
    return () => {
      cancelled = true;
    };
  }, [isEdit, prefillAsset, prefillEquipmentId]);

  // ── Validation ────────────────────────────────────────────────────────

  /** Create flow always requires an explicit type; edit only when the catalog has rows. */
  const requireType = !isEdit || woTypes.length > 0;
  const requireUrgency = urgencyLevels.length > 0;
  const requireEquipment = !isEdit;
  const urgencyIdValid = !requireUrgency || urgencyLevels.some((l) => l.code === urgencyId.trim());
  const selectedEquipmentId =
    selectedAsset != null && selectedAsset.id > 0 ? selectedAsset.id : null;

  const currentErrors = useMemo(
    () =>
      validate(
        { title, typeCode, urgencyId, urgencyIdValid, equipmentId: selectedEquipmentId },
        { requireType, requireUrgency, requireEquipment },
        t,
      ),
    [
      title,
      typeCode,
      urgencyId,
      urgencyIdValid,
      selectedEquipmentId,
      requireType,
      requireUrgency,
      requireEquipment,
      t,
    ],
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
    setTouched(new Set(["title", "typeCode", "urgencyId", "equipment"]));
    setSubmitError(null);
    if (!isValid || !catalogsReady || !info?.user_id) return;

    const resolvedEquipmentId = selectedEquipmentId;

    try {
      if (isEdit && initial) {
        const draftPayload = {
          id: initial.id,
          expected_row_version: initial.row_version,
          title: title.trim(),
          description: description.trim() || null,
          type_code: typeCode || null,
          equipment_id: resolvedEquipmentId,
          urgency_code: urgencyId ? (urgencyId as UrgencyCode) : null,
          subtype_code: subtypeCode,
          planned_start: plannedStart ? fromDateTimeLocal(plannedStart) : null,
          planned_end: plannedEnd ? fromDateTimeLocal(plannedEnd) : null,
          expected_duration_hours: expectedDuration,
          notes: notes.trim() || null,
          requires_permit: requiresPermit,
        };
        await updateDraft(draftPayload);
        onSubmitted({ ...initial, title, description, requires_permit: requiresPermit });
      } else {
        const input: WoCreateInput = {
          title: title.trim(),
          description: description.trim() || null,
          type_code: typeCode.trim(),
          equipment_id: resolvedEquipmentId,
          urgency_code: urgencyId ? (urgencyId as UrgencyCode) : null,
          subtype_code: subtypeCode || null,
          planned_start: plannedStart ? fromDateTimeLocal(plannedStart) : null,
          planned_end: plannedEnd ? fromDateTimeLocal(plannedEnd) : null,
          expected_duration_hours: expectedDuration,
          notes: notes.trim() || null,
          creator_id: info.user_id,
          requires_permit: requiresPermit,
        };
        const wo = await submitNewWo(input);
        onSubmitted(wo);
      }
    } catch (err) {
      const described = describeAppError(err);
      setSubmitError(
        described.description ? `${described.title} — ${described.description}` : described.title,
      );
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
    selectedEquipmentId,
    urgencyId,
    subtypeCode,
    plannedStart,
    plannedEnd,
    expectedDuration,
    notes,
    requiresPermit,
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
              <SelectValue
                placeholder={typesLoading ? t("form.type.loading") : t("form.type.label")}
              />
            </SelectTrigger>
            <SelectContent>
              {woTypes.map((wt) => {
                const nature = woNatureLabel(wt, i18n.language);
                return (
                  <SelectItem key={wt.id} value={wt.code}>
                    <span className="flex flex-col">
                      <span>{woTypeLabel(wt, i18n.language)}</span>
                      {nature ? (
                        <span className="text-[11px] text-text-muted">
                          {t("form.type.nature", { nature })}
                        </span>
                      ) : null}
                    </span>
                  </SelectItem>
                );
              })}
            </SelectContent>
          </Select>
        </FormField>

        {/* Sub-type (plant list, under the type's nature) */}
        {subtypeOptions.length > 0 ? (
          <FormField name="subtype" label={t("form.subtype.label")}>
            <Select
              value={subtypeCode || NO_SUBTYPE}
              onValueChange={(v) => setSubtypeCode(v === NO_SUBTYPE ? "" : v)}
            >
              <SelectTrigger id="wo-subtype" className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_SUBTYPE}>{t("form.subtype.none")}</SelectItem>
                {subtypeOptions.map((st) => (
                  <SelectItem key={st.id} value={st.code}>
                    {st.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
        ) : null}

        {/* Equipment */}
        <FormField
          name="equipment"
          label={t("form.equipment.label")}
          error={fieldError("equipment")}
          required={requireEquipment}
        >
          <AssetPicker
            id="wo-equipment-search"
            value={selectedAsset}
            onChange={(asset) => {
              setSelectedAsset(asset);
              markTouched("equipment");
            }}
            error={null}
          />
        </FormField>

        {/* Urgency */}
        <FormField
          name="urgency"
          label={t("form.urgency.label")}
          error={fieldError("urgencyId")}
          required={requireUrgency}
        >
          <UrgencySelect
            id="wo-urgency"
            value={urgencyId || null}
            onChange={(code) => {
              setUrgencyId(code);
              markTouched("urgencyId");
            }}
            placeholder={t("form.urgency.label")}
            className="w-full"
            aria-invalid={Boolean(fieldError("urgencyId"))}
          />
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

        {/* Safety: permit / LOTO gate on start */}
        <WoPermitRequirementField
          checked={requiresPermit}
          onCheckedChange={setRequiresPermit}
          disabled={permitLocked}
          disabledReason={permitLockedReason}
        />
      </section>

      <Separator />

      {/* ── Section 2: Planning ──────────────────────────────────────── */}
      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-text-primary">{t("detail.sections.planning")}</h3>

        <div className="grid grid-cols-2 gap-4">
          {/* Planned Start */}
          <FormField name="plannedStart" label={t("form.plannedStart.label")}>
            <DateTimeField
              id="wo-planned-start"
              value={plannedStart}
              onChange={(v) => setPlannedStart(v)}
            />
          </FormField>

          {/* Planned End */}
          <FormField name="plannedEnd" label={t("form.plannedEnd.label")}>
            <DateTimeField
              id="wo-planned-end"
              value={plannedEnd}
              min={plannedStart || undefined}
              onChange={(v) => setPlannedEnd(v)}
            />
          </FormField>
        </div>

        {/* Expected Duration */}
        <FormField name="duration" label={t("form.duration.label")}>
          <DecimalField
            id="wo-duration"
            min={0}
            value={expectedDuration}
            onValueChange={setExpectedDuration}
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
        <div className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg">
          {[submitError, storeError, catalogError, prioritiesError]
            .filter((e): e is string => Boolean(e))
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
