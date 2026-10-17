/**
 * DiCreateForm.tsx
 *
 * Create / edit form of an intervention request: requester (read-only), equipment and its
 * location in the plant hierarchy, what was observed (symptom from the shared ISO 14224 list,
 * urgency with the plain-language target of each level), pictures and files.
 * Before the request is sent, open requests already raised on the same asset are shown (same
 * symptom first) so the requester can avoid a duplicate. Everything is read from the local
 * database, so the form works offline.
 */

import { AlertTriangle, Loader2, MapPin } from "lucide-react";
import { type ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { UrgencySelect } from "@/components/common/urgency/UrgencySelect";
import { urgencyLevelLabel, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import { DiAttachmentPanel } from "@/components/di/DiAttachmentPanel";
import { diStatusToI18nKey } from "@/components/di/status-meta";
import {
  EntityFormAttachments,
  EntityFormImageUploader,
  orderImagesForUpload,
  pickEntityFormFiles,
  pickEntityFormImages,
  type EntityFormAttachmentItem,
  type EntityFormFileItem,
  type EntityFormImageItem,
} from "@/components/entity-form";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Separator } from "@/components/ui/separator";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { DI_STATUS_TONE, mfTone, toneOf } from "@/design-system/tone";
import { useSession } from "@/hooks/use-session";
import { describeAppError } from "@/lib/app-error";
import { assetToSearchResult } from "@/lib/asset-to-search-result";
import { cn } from "@/lib/utils";
import { getAssetById, getAssetByIdSilent } from "@/services/asset-service";
import { uploadDiAttachmentFromPath } from "@/services/di-attachment-service";
import { findPossibleDuplicateDis, type PossibleDuplicate } from "@/services/di-service";
import { useDiStore } from "@/stores/di-store";
import type { AssetSearchResult, DiCreateInput, InterventionRequest } from "@shared/ipc-types";

// ── Constants ─────────────────────────────────────────────────────────────────

const TITLE_MAX = 100;
const DESC_MAX = 1000;
/** Possible duplicates listed before submitting (the most relevant first). */
const DUPLICATES_SHOWN = 5;

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiCreateFormProps {
  initial: InterventionRequest | null;
  /** Prefill equipment snapshot when creating from asset context. */
  prefillAsset?: AssetSearchResult | null;
  /** Legacy id-only prefill (e.g. deep links). */
  prefillEquipmentId?: number | null;
  onSubmitted: (di: InterventionRequest) => void;
  onCancel: () => void;
}

// ── Validation ────────────────────────────────────────────────────────────────

interface FormErrors {
  title?: string;
  description?: string;
  origin_type?: string;
  request_type?: string;
  reported_urgency?: string;
  impact_level?: string;
  symptom_code_id?: string;
  equipment?: string;
}

function validate(
  fields: {
    title: string;
    description: string;
    origin_type: string;
    request_type: string;
    reported_urgency: string;
    impact_level: string;
    symptomCodeId: number | null;
    assetId: number | null;
  },
  t: (key: string) => string,
): FormErrors {
  const errors: FormErrors = {};
  if (!fields.title.trim()) errors.title = t("form.validation.titleRequired");
  if (!fields.description.trim()) errors.description = t("form.validation.descriptionRequired");
  if (!fields.origin_type) errors.origin_type = t("form.validation.originRequired");
  if (!fields.request_type) errors.request_type = t("form.validation.requestTypeRequired");
  if (!fields.reported_urgency) errors.reported_urgency = t("form.validation.urgencyRequired");
  if (!fields.impact_level) errors.impact_level = t("form.validation.impactRequired");
  if (!fields.symptomCodeId) errors.symptom_code_id = t("form.validation.symptomRequired");
  if (!fields.assetId) errors.equipment = t("form.validation.equipmentRequired");
  return errors;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiCreateForm({
  initial,
  prefillAsset = null,
  prefillEquipmentId = null,
  onSubmitted,
  onCancel,
}: DiCreateFormProps) {
  const { t } = useTranslation("di");
  const { info } = useSession();
  const saving = useDiStore((s) => s.saving);
  const storeError = useDiStore((s) => s.error);
  const submitNewDi = useDiStore((s) => s.submitNewDi);
  const updateDraft = useDiStore((s) => s.updateDraft);

  const [submitError, setSubmitError] = useState<string | null>(null);
  const [uploadingMedia, setUploadingMedia] = useState(false);

  const isEdit = initial !== null;

  // ── Form state ────────────────────────────────────────────────────────

  const [title, setTitle] = useState(initial?.title ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [originType, setOriginType] = useState<string>(initial?.origin_type ?? "");
  const [requestType, setRequestType] = useState<string>(initial?.request_type ?? "");
  const [urgency, setUrgency] = useState<string>(initial?.reported_urgency ?? "");
  const [impactLevel, setImpactLevel] = useState<string>(initial?.impact_level ?? "");
  const [symptomCodeId, setSymptomCodeId] = useState<number | null>(
    initial?.symptom_code_id ?? null,
  );
  const [observedAt, setObservedAt] = useState(initial?.observed_at ?? "");
  const [safetyFlag, setSafetyFlag] = useState(initial?.safety_flag ?? false);
  const [environmentalFlag, setEnvironmentalFlag] = useState(initial?.environmental_flag ?? false);
  const [qualityFlag, setQualityFlag] = useState(initial?.quality_flag ?? false);
  const [productionImpact, setProductionImpact] = useState(initial?.production_impact ?? false);
  const [pendingPhotos, setPendingPhotos] = useState<EntityFormImageItem[]>([]);
  const [pendingFiles, setPendingFiles] = useState<EntityFormFileItem[]>([]);

  // Equipment selection (shared AssetPicker)
  const [selectedAsset, setSelectedAsset] = useState<AssetSearchResult | null>(null);

  // Open requests already raised on the same asset (duplicate warning)
  const [duplicates, setDuplicates] = useState<PossibleDuplicate[]>([]);
  const { levelOf, language } = useUrgencyScale();
  const urgencyLevel = levelOf(urgency);

  // Validation
  const [touched, setTouched] = useState<Set<string>>(new Set());
  const [, setErrors] = useState<FormErrors>({});

  // ── Equipment pre-fill for edit mode ──────────────────────────────────

  useEffect(() => {
    if (initial?.equipment_id && !selectedAsset) {
      void getAssetById(initial.equipment_id)
        .then((asset) => setSelectedAsset(assetToSearchResult(asset)))
        .catch(() => {
          /* Missing asset — user can search manually */
        });
    }
  }, [initial, selectedAsset]);

  // ── New DI: one-shot pre-fill from asset context ──────────────────────

  const prefillConsumedRef = useRef(false);
  useEffect(() => {
    if (isEdit) return;
    if (prefillConsumedRef.current) return;

    if (prefillAsset) {
      prefillConsumedRef.current = true;
      setSelectedAsset(prefillAsset);
      return;
    }

    if (prefillEquipmentId == null || prefillEquipmentId <= 0) {
      return;
    }

    prefillConsumedRef.current = true;
    let cancelled = false;
    void getAssetByIdSilent(prefillEquipmentId).then((asset) => {
      if (!cancelled && asset) setSelectedAsset(assetToSearchResult(asset));
    });
    return () => {
      cancelled = true;
    };
  }, [isEdit, prefillAsset, prefillEquipmentId]);

  // ── Duplicate warning (same asset, open, same symptom first) ─────────

  const assetId = selectedAsset?.id ?? null;
  useEffect(() => {
    if (assetId == null) {
      setDuplicates([]);
      return;
    }
    let cancelled = false;
    void findPossibleDuplicateDis({
      equipmentId: assetId,
      symptomCodeId,
      excludeId: initial?.id ?? null,
    })
      .then((found) => {
        if (!cancelled) setDuplicates(found);
      })
      .catch(() => {
        // The warning is a help, not a gate: no list when it cannot be read.
        if (!cancelled) setDuplicates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [assetId, symptomCodeId, initial?.id]);

  // ── Validation ────────────────────────────────────────────────────────

  const currentErrors = useMemo(
    () =>
      validate(
        {
          title,
          description,
          origin_type: originType,
          request_type: requestType,
          reported_urgency: urgency,
          impact_level: impactLevel,
          symptomCodeId: symptomCodeId,
          assetId: selectedAsset?.id ?? null,
        },
        (key) => t(key as never),
      ),
    [
      title,
      description,
      originType,
      requestType,
      urgency,
      impactLevel,
      symptomCodeId,
      selectedAsset,
      t,
    ],
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
        "request_type",
        "reported_urgency",
        "impact_level",
        "symptom_code_id",
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
          request_type: requestType,
          impact_level: impactLevel,
          symptom_code_id: symptomCodeId,
          production_impact: productionImpact,
          safety_flag: safetyFlag,
          environmental_flag: environmentalFlag,
          quality_flag: qualityFlag,
          reported_urgency: urgency,
          observed_at: observedAt || null,
        });
        const updated = useDiStore.getState().activeDi?.di;
        onSubmitted(updated && updated.id === initial.id ? updated : initial);
      } else {
        const input: DiCreateInput = {
          equipment_id: selectedAsset.id,
          org_node_id: selectedAsset.org_node_id,
          title: title.trim(),
          description: description.trim(),
          origin_type: originType,
          request_type: requestType,
          symptom_code_id: symptomCodeId,
          impact_level: impactLevel,
          production_impact: productionImpact,
          safety_flag: safetyFlag,
          environmental_flag: environmentalFlag,
          quality_flag: qualityFlag,
          reported_urgency: urgency,
          observed_at: observedAt || null,
          source_inspection_anomaly_id: null,
          submitter_id: info.user_id,
        };
        const di = await submitNewDi(input);

        const photos = orderImagesForUpload(pendingPhotos).filter((p) => Boolean(p.path));
        const files = pendingFiles.filter((f) => Boolean(f.path));
        if (photos.length > 0 || files.length > 0) {
          setUploadingMedia(true);
          let failed = 0;
          try {
            for (const photo of photos) {
              try {
                if (!photo.path) {
                  failed += 1;
                  continue;
                }
                await uploadDiAttachmentFromPath({
                  diId: di.id,
                  sourcePath: photo.path,
                  attachmentType: "photo",
                });
              } catch {
                failed += 1;
              }
            }
            for (const file of files) {
              try {
                await uploadDiAttachmentFromPath({
                  diId: di.id,
                  sourcePath: file.path,
                  attachmentType: null,
                });
              } catch {
                failed += 1;
              }
            }
          } finally {
            setUploadingMedia(false);
          }
          if (failed > 0) {
            setSubmitError(
              t("form.media.partialUploadFailed", {
                failed,
                total: photos.length + files.length,
              }),
            );
          }
        }

        onSubmitted(di);
      }
    } catch (err) {
      const described = describeAppError(err);
      setSubmitError(
        described.description ? `${described.title} — ${described.description}` : described.title,
      );
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
    requestType,
    urgency,
    impactLevel,
    symptomCodeId,
    observedAt,
    safetyFlag,
    environmentalFlag,
    qualityFlag,
    productionImpact,
    pendingPhotos,
    pendingFiles,
    submitNewDi,
    updateDraft,
    onSubmitted,
    t,
  ]);

  const busy = saving || uploadingMedia;

  const attachmentListItems: EntityFormAttachmentItem[] = pendingFiles.map((f) => ({
    id: f.id,
    name: f.name,
  }));

  const handleAddFiles = useCallback(async () => {
    const picked = await pickEntityFormFiles();
    if (picked.length > 0) setPendingFiles((prev) => [...prev, ...picked]);
  }, []);

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
        <AssetPicker
          id="equipment-search"
          value={selectedAsset}
          onChange={(asset) => {
            setSelectedAsset(asset);
            markTouched("equipment");
          }}
          error={fieldError("equipment")}
          disabled={isEdit}
        />
        {selectedAsset ? (
          <p className="mt-2 flex items-start gap-1.5 text-xs text-text-secondary">
            <MapPin className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
            <span>
              <span className="font-medium">{t("form.location.label")}</span>{" "}
              {selectedAsset.org_path ?? selectedAsset.org_node_name ?? t("form.location.none")}
              {selectedAsset.parent_asset_code ? (
                <>
                  {" · "}
                  {t("form.location.parent", {
                    parent: [selectedAsset.parent_asset_code, selectedAsset.parent_asset_name]
                      .filter(Boolean)
                      .join(" — "),
                  })}
                </>
              ) : null}
            </span>
          </p>
        ) : null}
        {duplicates.length > 0 ? (
          <div
            role="status"
            className={cn("mt-3 space-y-1.5 rounded-lg border px-3 py-2", mfTone.attention.panel)}
          >
            <p className="flex items-center gap-1.5 text-xs font-semibold text-text-primary">
              <AlertTriangle className="h-3.5 w-3.5 shrink-0" aria-hidden />
              {duplicates.some((d) => d.sameSymptom)
                ? t("form.duplicates.sameSymptom", {
                    count: duplicates.filter((d) => d.sameSymptom).length,
                  })
                : t("form.duplicates.sameAsset", { count: duplicates.length })}
            </p>
            <ul className="space-y-1">
              {duplicates.slice(0, DUPLICATES_SHOWN).map(({ di, sameSymptom }) => (
                <li key={di.id} className="flex items-center gap-2 text-xs">
                  <span className="font-mono text-text-muted">{di.code}</span>
                  <span className={cn("min-w-0 flex-1 truncate", sameSymptom && "font-medium")}>
                    {di.title}
                  </span>
                  <StatusChip
                    label={t(`status.${diStatusToI18nKey(di.status)}` as const)}
                    tone={toneOf(DI_STATUS_TONE, di.status)}
                    dot
                  />
                </li>
              ))}
            </ul>
            <p className="text-xs text-text-secondary">{t("form.duplicates.hint")}</p>
          </div>
        ) : null}
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
          <ReferenceCombobox
            id="origin_type"
            referenceType="di.origin"
            value={originType || null}
            onChange={(code) => {
              setOriginType(code ?? "");
              markTouched("origin_type");
            }}
            allowClear={false}
            disabled={isEdit}
            placeholder={t("form.originType.placeholder")}
            aria-invalid={Boolean(fieldError("origin_type"))}
          />
        </FormField>

        <FormField
          name="request_type"
          label={t("form.requestType.label")}
          error={fieldError("request_type")}
          required
        >
          <ReferenceCombobox
            id="request_type"
            referenceType="di.request_type"
            value={requestType || null}
            onChange={(code) => {
              setRequestType(code ?? "");
              markTouched("request_type");
            }}
            allowClear={false}
            allowCreate={false}
            placeholder={t("form.requestType.placeholder")}
            aria-invalid={Boolean(fieldError("request_type"))}
          />
        </FormField>

        <FormField
          name="symptom_code_id"
          label={t("form.symptom.label")}
          error={fieldError("symptom_code_id")}
          required
        >
          <ReferenceCombobox
            id="symptom_code_id"
            referenceType="work.symptom"
            valueMode="id"
            value={symptomCodeId != null ? String(symptomCodeId) : null}
            onChange={(idStr) => {
              setSymptomCodeId(idStr ? Number(idStr) : null);
              markTouched("symptom_code_id");
            }}
            allowClear={false}
            placeholder={t("form.symptom.placeholder")}
            aria-invalid={Boolean(fieldError("symptom_code_id"))}
          />
        </FormField>

        {/* Urgency */}
        <FormField
          name="reported_urgency"
          label={t("form.priority.label")}
          error={fieldError("reported_urgency")}
          required
        >
          <UrgencySelect
            id="reported_urgency"
            value={urgency || null}
            onChange={(code) => {
              setUrgency(code);
              markTouched("reported_urgency");
            }}
            placeholder={t("form.priority.placeholder")}
            aria-invalid={Boolean(fieldError("reported_urgency"))}
            aria-describedby="reported_urgency-hint"
          />
          <p id="reported_urgency-hint" className="mt-1 text-xs text-text-secondary">
            {urgencyLevel
              ? [
                  urgencyLevel.description,
                  urgencyLevel.response_target_hours != null &&
                  urgencyLevel.completion_target_hours != null
                    ? t("form.priority.targetHint", {
                        level: urgencyLevelLabel(urgencyLevel, language),
                        response: urgencyLevel.response_target_hours,
                        completion: urgencyLevel.completion_target_hours,
                      })
                    : t("form.priority.noTarget"),
                ]
                  .filter(Boolean)
                  .join(" ")
              : t("form.priority.chooseHint")}
          </p>
        </FormField>

        {/* Impact level */}
        <FormField
          name="impact_level"
          label={t("form.impactLevel.label")}
          error={fieldError("impact_level")}
          required
        >
          <ReferenceCombobox
            id="impact_level"
            referenceType="di.impact"
            value={impactLevel || null}
            onChange={(code) => {
              setImpactLevel(code ?? "");
              markTouched("impact_level");
            }}
            allowClear={false}
            allowCreate={false}
            placeholder={t("form.impactLevel.placeholder")}
            aria-invalid={Boolean(fieldError("impact_level"))}
          />
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

      <Separator />

      {/* ── Media: pictures + attachments ───────────────────────────── */}
      <section className="space-y-4">
        <h3 className="text-sm font-semibold text-text-primary mb-1">{t("form.section.media")}</h3>

        {isEdit && initial ? (
          <DiAttachmentPanel diId={initial.id} canUpload={true} canDelete={false} />
        ) : (
          <>
            <div className="space-y-2">
              <p className="text-xs font-medium text-text-secondary">{t("form.pictures.label")}</p>
              <EntityFormImageUploader
                items={pendingPhotos}
                onChange={setPendingPhotos}
                onPickFiles={pickEntityFormImages}
                disabled={busy}
                uploading={uploadingMedia}
                emptyLabel={t("form.pictures.empty")}
                hintLabel={t("form.pictures.hint")}
                primaryLabel={t("form.pictures.primary")}
                removeLabel={t("form.pictures.remove")}
                addLabel={t("form.pictures.add")}
              />
            </div>

            <div className="space-y-2">
              <p className="text-xs font-medium text-text-secondary">
                {t("form.attachments.label")}
              </p>
              <EntityFormAttachments
                items={attachmentListItems}
                onAdd={() => void handleAddFiles()}
                onRemove={(id) => setPendingFiles((prev) => prev.filter((f) => f.id !== id))}
                disabled={busy}
                emptyLabel={t("form.attachments.empty")}
                addLabel={t("form.attachments.add")}
                removeLabel={t("form.attachments.remove")}
              />
            </div>
          </>
        )}
      </section>

      {/* ── Error banner ────────────────────────────────────────────── */}
      {(submitError || storeError) && (
        <div
          role="alert"
          className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
        >
          {submitError || storeError}
        </div>
      )}

      {/* ── Footer ───────────────────────────────────────────────────── */}
      <div className="flex items-center justify-end gap-2 pt-2 border-t border-surface-border">
        <Button variant="outline" onClick={onCancel} disabled={busy}>
          {t("form.cancel")}
        </Button>
        <Button
          onClick={() => void handleSubmit()}
          disabled={busy || (!isValid && touched.size > 0)}
        >
          {busy && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
          {isEdit ? t("form.update") : t("form.submit")}
        </Button>
      </div>
    </div>
  );
}
