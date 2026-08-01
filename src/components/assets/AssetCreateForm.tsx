/**
 * AssetCreateForm.tsx
 *
 * Reference implementation of the Entity Form Dialog design system.
 * @see docs/UX_ENTITY_FORM_DIALOG_PATTERN.md
 */

import { CircleHelp, Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { EquipmentParentPicker } from "@/components/assets/EquipmentParentPicker";
import {
  EquipmentSearchSelect,
  type EquipmentSearchOption,
} from "@/components/assets/EquipmentSearchSelect";
import {
  EntityFormAttachments,
  EntityFormCollapsible,
  EntityFormDialog,
  EntityFormFieldGroup,
  EntityFormFooter,
  EntityFormHint,
  EntityFormImageUploader,
  EntityFormSection,
  orderImagesForUpload,
  pickEntityFormImages,
  type EntityFormImageItem,
} from "@/components/entity-form";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { FormField } from "@/components/ui/FormField";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useToast } from "@/hooks/use-toast";
import { useZodForm } from "@/lib/form-helpers";
import { assetCreateSchema } from "@/schemas/asset-create.schema";
import { uploadAssetPhoto } from "@/services/asset-lifecycle-service";
import { getEquipmentTaxonomyCatalog } from "@/services/asset-service";
import { listOrgTree } from "@/services/org-node-service";
import { useAssetStore } from "@/stores/asset-store";
import { toErrorMessage } from "@/utils/errors";
import type { CreateAssetPayload, OrgTreeRow } from "@shared/ipc-types";

const FORM_ID = "asset-create-form";

function translateFieldError(
  message: string | undefined,
  translate: (key: string) => unknown,
): string | undefined {
  return message ? String(translate(message)) : undefined;
}

export function AssetCreateForm() {
  const { t } = useTranslation("equipment");
  const { toast } = useToast();
  const open = useAssetStore((s) => s.showCreateForm);
  const closeForm = useAssetStore((s) => s.closeCreateForm);
  const submitCreate = useAssetStore((s) => s.createAsset);
  const saving = useAssetStore((s) => s.saving);
  const storeError = useAssetStore((s) => s.error);
  const parentPreFill = useAssetStore((s) => s.parentPreFill);

  const fieldError = (message: string | undefined) => translateFieldError(message, t);

  const [submitError, setSubmitError] = useState<string | null>(null);
  const [catalogReady, setCatalogReady] = useState(false);
  const [orgNodes, setOrgNodes] = useState<OrgTreeRow[]>([]);
  const [orgTreeError, setOrgTreeError] = useState<string | null>(null);
  const [referenceCatalogError, setReferenceCatalogError] = useState<string | null>(null);
  const [pendingPhotos, setPendingPhotos] = useState<EntityFormImageItem[]>([]);
  const [uploadingPhotos, setUploadingPhotos] = useState(false);

  const form = useZodForm(assetCreateSchema, {
    asset_code: "",
    asset_name: "",
    class_code: "",
    family_code: null,
    subfamily_code: null,
    criticality_code: "",
    status_code: "",
    manufacturer: null,
    model: null,
    serial_number: null,
    maintainable_boundary: true,
    org_node_id: 0 as unknown as number,
    parent_asset_id: parentPreFill?.id ?? null,
    commissioned_at: null,
    rams_schedule_reference_value_id: null,
    rams_utilization_factor: 1.0,
    description: null,
  });

  const {
    register,
    handleSubmit,
    setValue,
    watch,
    reset,
    formState: { errors },
  } = form;

  useEffect(() => {
    if (!open) return;
    setSubmitError(null);
    setOrgTreeError(null);
    setReferenceCatalogError(null);
    setCatalogReady(false);
    setPendingPhotos([]);
    void listOrgTree()
      .then(setOrgNodes)
      .catch((err) => {
        console.error("[AssetCreateForm] listOrgTree failed", err);
        setOrgTreeError(`${t("createForm.orgTreeLoadFailed")} (${toErrorMessage(err)})`);
      });
    void getEquipmentTaxonomyCatalog()
      .then(() => setCatalogReady(true))
      .catch((err) => {
        console.error("[AssetCreateForm] getEquipmentTaxonomyCatalog failed", err);
        setReferenceCatalogError(
          `${t("createForm.referenceCatalogLoadFailed")} (${toErrorMessage(err)})`,
        );
        setCatalogReady(false);
      });
  }, [open, t]);

  useEffect(() => {
    if (open && parentPreFill) {
      setValue("parent_asset_id", parentPreFill.id);
    }
  }, [open, parentPreFill, setValue]);

  useEffect(() => {
    if (!open) return;
    reset({
      asset_code: "",
      asset_name: "",
      class_code: "",
      family_code: null,
      subfamily_code: null,
      criticality_code: "",
      status_code: "",
      manufacturer: null,
      model: null,
      serial_number: null,
      maintainable_boundary: true,
      org_node_id: 0 as unknown as number,
      parent_asset_id: parentPreFill?.id ?? null,
      commissioned_at: null,
      rams_schedule_reference_value_id: null,
      rams_utilization_factor: 1.0,
      description: null,
    });
  }, [open, parentPreFill, reset]);

  const orgOptions: EquipmentSearchOption[] = useMemo(
    () =>
      orgNodes.map((n) => ({
        value: String(n.node.id),
        label: n.node.name,
        description: n.node.code,
      })),
    [orgNodes],
  );

  const watchedClassCode = watch("class_code");
  const watchedFamilyCode = watch("family_code");
  const prevClassRef = useRef<string | null>(null);
  const prevFamilyRef = useRef<string | null>(null);

  useEffect(() => {
    const prev = prevClassRef.current;
    prevClassRef.current = watchedClassCode || null;
    if (prev != null && prev !== (watchedClassCode || null)) {
      setValue("family_code", null, { shouldValidate: true });
      setValue("subfamily_code", null, { shouldValidate: true });
    }
  }, [watchedClassCode, setValue]);

  useEffect(() => {
    const prev = prevFamilyRef.current;
    const next = watchedFamilyCode ?? null;
    prevFamilyRef.current = next;
    if (prev != null && prev !== next) {
      setValue("subfamily_code", null, { shouldValidate: true });
    }
  }, [watchedFamilyCode, setValue]);

  const onSubmit: Parameters<typeof handleSubmit>[0] = async (values) => {
    setSubmitError(null);
    if (
      !values.class_code?.trim() ||
      !values.criticality_code?.trim() ||
      !values.status_code?.trim()
    ) {
      setSubmitError(t("createForm.validation.referenceRequired"));
      return;
    }
    if (!values.org_node_id || values.org_node_id === 0) {
      setSubmitError(t("createForm.validation.orgRequired"));
      return;
    }

    const payload: CreateAssetPayload = {
      asset_code: values.asset_code.trim(),
      asset_name: values.asset_name.trim(),
      class_code: values.class_code.trim(),
      criticality_code: values.criticality_code.trim(),
      status_code: values.status_code.trim(),
      maintainable_boundary: values.maintainable_boundary ?? true,
      org_node_id: values.org_node_id,
      ...(values.family_code ? { family_code: values.family_code } : {}),
      ...(values.subfamily_code ? { subfamily_code: values.subfamily_code } : {}),
      ...(values.manufacturer ? { manufacturer: values.manufacturer } : {}),
      ...(values.model ? { model: values.model } : {}),
      ...(values.serial_number ? { serial_number: values.serial_number } : {}),
      ...(values.commissioned_at ? { commissioned_at: values.commissioned_at } : {}),
      ...(values.rams_schedule_reference_value_id != null
        ? { rams_schedule_reference_value_id: values.rams_schedule_reference_value_id }
        : {}),
      rams_utilization_factor: values.rams_utilization_factor ?? 1.0,
    };

    try {
      const asset = await submitCreate(
        payload,
        values.parent_asset_id != null ? { parentAssetId: values.parent_asset_id } : {},
      );

      const toUpload = orderImagesForUpload(pendingPhotos).filter((p) => Boolean(p.path));
      if (toUpload.length > 0) {
        setUploadingPhotos(true);
        let failed = 0;
        try {
          for (const photo of toUpload) {
            try {
              await uploadAssetPhoto({
                asset_id: asset.id,
                source_path: photo.path!,
                caption: null,
              });
            } catch (err) {
              failed += 1;
              console.error("[AssetCreateForm] uploadAssetPhoto failed", err);
            }
          }
        } finally {
          setUploadingPhotos(false);
        }
        if (failed > 0) {
          toast({
            title: t("createForm.images.partialUploadFailed"),
            description: t("createForm.images.uploadFailedCount", {
              failed,
              total: toUpload.length,
            }),
            variant: "destructive",
          });
        }
      }

      closeForm();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setSubmitError(msg);
    }
  };

  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (!next && !saving && !uploadingPhotos) closeForm();
    },
    [closeForm, saving, uploadingPhotos],
  );

  const watchedOrgNode = watch("org_node_id");
  const busy = saving || uploadingPhotos;

  return (
    <EntityFormDialog
      open={open}
      onOpenChange={handleOpenChange}
      title={t("createForm.title")}
      description={t("createForm.description")}
      footer={
        <EntityFormFooter
          formId={FORM_ID}
          cancelLabel={t("decommission.cancel")}
          primaryLabel={
            <>
              {busy && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
              {t("createForm.submit")}
            </>
          }
          onCancel={() => closeForm()}
          cancelDisabled={busy}
          primaryDisabled={!catalogReady}
          primaryLoading={busy}
        />
      }
    >
      {(orgTreeError || referenceCatalogError) && (
        <div className="space-y-2">
          {orgTreeError && <EntityFormHint>{orgTreeError}</EntityFormHint>}
          {referenceCatalogError && <EntityFormHint>{referenceCatalogError}</EntityFormHint>}
        </div>
      )}

      <form
        id={FORM_ID}
        onSubmit={(e) => void handleSubmit(onSubmit)(e)}
        className="space-y-6"
      >
        <EntityFormSection title={t("createForm.sections.identification")}>
          <FormField
            name="asset_code"
            label={t("form.identity.code.label")}
            error={fieldError(errors.asset_code?.message)}
            required
          >
            <Input
              id="asset_code"
              placeholder={t("form.identity.code.placeholder")}
              autoComplete="off"
              {...register("asset_code")}
            />
          </FormField>
          <FormField
            name="asset_name"
            label={t("form.identity.name.label")}
            error={fieldError(errors.asset_name?.message)}
            required
          >
            <Input
              id="asset_name"
              placeholder={t("form.identity.name.placeholder")}
              {...register("asset_name")}
            />
          </FormField>
          <FormField name="description" label={t("createForm.descriptionLabel")}>
            <Textarea id="description" rows={3} maxLength={2000} {...register("description")} />
          </FormField>
        </EntityFormSection>

        <EntityFormSection title={t("createForm.sections.classification")}>
          <EntityFormFieldGroup>
            <FormField
              name="class_code"
              label={t("detail.fields.class")}
              error={fieldError(errors.class_code?.message)}
              required
            >
              <ReferenceCombobox
                id="class_code"
                referenceType="equipment.class"
                value={watch("class_code") || null}
                onChange={(v) => setValue("class_code", v ?? "", { shouldValidate: true })}
                placeholder={t("createForm.selectClass")}
                allowClear={false}
                aria-invalid={!!errors.class_code}
              />
            </FormField>
            <FormField name="family_code" label={t("detail.fields.family")}>
              <ReferenceCombobox
                id="family_code"
                referenceType="equipment.family"
                value={watch("family_code") ?? null}
                onChange={(v) => setValue("family_code", v, { shouldValidate: true })}
                parentCode={watchedClassCode || null}
                placeholder={t("createForm.selectFamilyOptional")}
                disabled={!watchedClassCode}
                allowClear
              />
            </FormField>
            <FormField name="subfamily_code" label={t("detail.fields.subfamily")}>
              <ReferenceCombobox
                id="subfamily_code"
                referenceType="equipment.subfamily"
                value={watch("subfamily_code") ?? null}
                onChange={(v) => setValue("subfamily_code", v, { shouldValidate: true })}
                parentCode={watchedFamilyCode ?? null}
                placeholder={t("createForm.selectSubfamilyOptional")}
                disabled={!watchedFamilyCode}
                allowClear
              />
            </FormField>
          </EntityFormFieldGroup>
          <FormField
            name="criticality_code"
            label={t("detail.fields.criticality")}
            error={fieldError(errors.criticality_code?.message)}
            required
          >
            <ReferenceCombobox
              id="criticality_code"
              referenceType="equipment.criticality"
              value={watch("criticality_code") || null}
              onChange={(v) => setValue("criticality_code", v ?? "", { shouldValidate: true })}
              placeholder={t("createForm.selectCriticality")}
              allowClear={false}
              aria-invalid={!!errors.criticality_code}
            />
          </FormField>
        </EntityFormSection>

        <EntityFormSection title={t("createForm.sections.organization")}>
          <FormField
            name="org_node_id"
            label={t("detail.fields.site")}
            error={fieldError(errors.org_node_id?.message)}
            required
          >
            <EquipmentSearchSelect
              id="org_node_id"
              options={orgOptions}
              value={watchedOrgNode ? String(watchedOrgNode) : ""}
              onChange={(v) => setValue("org_node_id", Number(v), { shouldValidate: true })}
              placeholder={t("createForm.selectOrg")}
              disabled={orgOptions.length === 0}
              aria-invalid={!!errors.org_node_id}
            />
          </FormField>
          <FormField name="parent_asset_id" label={t("createForm.parentAsset")}>
            <EquipmentParentPicker
              id="parent_asset_id"
              value={watch("parent_asset_id") ?? null}
              onChange={(id) => setValue("parent_asset_id", id, { shouldValidate: true })}
              rootAssetId={parentPreFill?.id ?? null}
            />
          </FormField>
        </EntityFormSection>

        <EntityFormSection title={t("createForm.sections.technical")}>
          <FormField name="manufacturer" label={t("detail.fields.manufacturer")}>
            <Input id="manufacturer" {...register("manufacturer")} />
          </FormField>
          <FormField name="model" label={t("detail.fields.model")}>
            <Input id="model" {...register("model")} />
          </FormField>
          <FormField name="serial_number" label={t("detail.fields.serialNumber")}>
            <Input id="serial_number" {...register("serial_number")} />
          </FormField>
          <div className="flex items-center gap-2">
            <Checkbox
              id="maintainable_boundary"
              checked={watch("maintainable_boundary")}
              onCheckedChange={(c) => setValue("maintainable_boundary", c === true)}
            />
            <Label htmlFor="maintainable_boundary" className="text-sm">
              {t("createForm.maintainableBoundary")}
            </Label>
          </div>
        </EntityFormSection>

        <EntityFormSection title={t("createForm.sections.lifecycle")}>
          <FormField
            name="status_code"
            label={t("detail.fields.status")}
            error={fieldError(errors.status_code?.message)}
            required
          >
            <ReferenceCombobox
              id="status_code"
              referenceType="equipment.status"
              value={watch("status_code") || null}
              onChange={(v) => setValue("status_code", v ?? "", { shouldValidate: true })}
              placeholder={t("createForm.selectStatus")}
              allowClear={false}
              aria-invalid={!!errors.status_code}
            />
          </FormField>
          <FormField name="commissioned_at" label={t("detail.fields.commissioningDate")}>
            <Input id="commissioned_at" type="date" {...register("commissioned_at")} />
          </FormField>
        </EntityFormSection>

        <EntityFormSection title={t("createForm.sections.images")}>
          <EntityFormImageUploader
            items={pendingPhotos}
            onChange={setPendingPhotos}
            onPickFiles={pickEntityFormImages}
            disabled={busy}
            uploading={uploadingPhotos}
            emptyLabel={t("createForm.images.empty")}
            hintLabel={t("createForm.images.hint")}
            primaryLabel={t("createForm.images.primary")}
            removeLabel={t("createForm.images.remove")}
            addLabel={t("createForm.images.add")}
          />
        </EntityFormSection>

        <EntityFormSection title={t("createForm.sections.attachments")}>
          <EntityFormAttachments
            items={[]}
            deferred
            deferredLabel={t("createForm.attachments.deferred")}
          />
        </EntityFormSection>

        <EntityFormCollapsible
          title={
            <span className="inline-flex items-center gap-1.5">
              {t("createForm.sections.rams")}
              <CircleHelp className="h-3.5 w-3.5 text-text-muted" aria-hidden />
            </span>
          }
          description={t("createForm.sections.ramsDescription")}
          defaultClosed
        >
          <FormField
            name="rams_schedule_reference_value_id"
            label={t("createForm.fields.ramsScheduleClass")}
          >
            <ReferenceCombobox
              id="rams_schedule_reference_value_id"
              referenceType="org.schedule_class"
              valueMode="id"
              value={
                watch("rams_schedule_reference_value_id") != null
                  ? String(watch("rams_schedule_reference_value_id"))
                  : null
              }
              onChange={(v) =>
                setValue(
                  "rams_schedule_reference_value_id",
                  v != null && v !== "" ? Number(v) : null,
                  { shouldValidate: true },
                )
              }
              placeholder={t("createForm.selectScheduleClass")}
              allowClear
            />
          </FormField>
          <FormField
            name="rams_utilization_factor"
            label="Utilization Factor (K_u)"
            error={errors.rams_utilization_factor?.message}
          >
            <Input
              id="rams_utilization_factor"
              type="number"
              min={0.01}
              max={1.5}
              step={0.01}
              {...register("rams_utilization_factor", { valueAsNumber: true })}
            />
            <p className="mt-1 text-[11px] text-text-muted">
              Typical value is 1.00. Lower values model intermittent utilization.
            </p>
          </FormField>
        </EntityFormCollapsible>

        {(submitError || storeError) && (
          <div className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
            {submitError || storeError}
          </div>
        )}
      </form>
    </EntityFormDialog>
  );
}
