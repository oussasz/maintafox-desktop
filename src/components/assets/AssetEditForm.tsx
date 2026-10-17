/**
 * AssetEditForm.tsx
 *
 * Reference implementation of the Entity Form Dialog design system (edit).
 * @see docs/UX_ENTITY_FORM_DIALOG_PATTERN.md
 */

import { CircleHelp, Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  EquipmentSearchSelect,
  type EquipmentSearchOption,
} from "@/components/assets/EquipmentSearchSelect";
import { isRetiredStatus } from "@/components/assets/asset-tones";
import {
  EntityFormCollapsible,
  EntityFormDialog,
  EntityFormFieldGroup,
  EntityFormFooter,
  EntityFormHint,
  EntityFormImageUploader,
  EntityFormSection,
  pickEntityFormImages,
  type EntityFormImageItem,
} from "@/components/entity-form";
import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { useFormatters } from "@/hooks/use-formatters";
import { useToast } from "@/hooks/use-toast";
import { appErrorReason } from "@/lib/app-error";
import { useZodForm } from "@/lib/form-helpers";
import { useCanReadList } from "@/lib/list-access";
import { assetEditSchema } from "@/schemas/asset-edit.schema";
import {
  deleteAssetPhoto,
  listAssetPhotos,
  readAssetPhotoPreview,
  uploadAssetPhoto,
} from "@/services/asset-lifecycle-service";
import { getEquipmentTaxonomyCatalog } from "@/services/asset-service";
import { listOrgTree } from "@/services/org-node-service";
import { useAssetStore } from "@/stores/asset-store";
import { useEquipmentTaxonomy } from "@/stores/equipment-taxonomy-store";
import type { OrgTreeRow, UpdateAssetIdentityPayload } from "@shared/ipc-types";

const FORM_ID = "asset-edit-form";

function translateFieldError(
  message: string | undefined,
  translate: (key: string) => unknown,
): string | undefined {
  return message ? String(translate(message)) : undefined;
}

function photoDataUrl(preview: { mime_type: string; data_base64: string }): string {
  return `data:${preview.mime_type};base64,${preview.data_base64}`;
}

export function AssetEditForm() {
  const { t } = useTranslation("equipment");
  const canReadOrg = useCanReadList("orgNodes");
  const { toast } = useToast();
  const open = useAssetStore((s) => s.showEditForm);
  const asset = useAssetStore((s) => s.editingAsset);
  const closeForm = useAssetStore((s) => s.closeEditForm);
  const submitUpdate = useAssetStore((s) => s.updateAsset);
  const saving = useAssetStore((s) => s.saving);
  const { formatDate, formatDateTime } = useFormatters();
  const { catalog: taxonomy } = useEquipmentTaxonomy();
  /** Retired statuses are set only by Decommission (it checks open work and records why). */
  const retiredStatusCodes = useMemo(
    () =>
      (taxonomy?.statuses ?? [])
        .filter((s) => isRetiredStatus(taxonomy, s.code))
        .map((s) => s.code),
    [taxonomy],
  );
  const assetRetired = isRetiredStatus(taxonomy, asset?.status_code);

  const fieldError = (message: string | undefined) => translateFieldError(message, t);

  const [catalogReady, setCatalogReady] = useState(false);
  const [orgNodes, setOrgNodes] = useState<OrgTreeRow[]>([]);
  const [orgTreeError, setOrgTreeError] = useState<string | null>(null);
  const [referenceCatalogError, setReferenceCatalogError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [showUnsavedPrompt, setShowUnsavedPrompt] = useState(false);
  const [photoItems, setPhotoItems] = useState<EntityFormImageItem[]>([]);
  const [photosBusy, setPhotosBusy] = useState(false);
  /** The photo list could not be read: shown with a retry instead of an empty gallery. */
  const [photosError, setPhotosError] = useState<string | null>(null);
  /** Photos the user removed in the gallery: deleted only once confirmed (deletion is immediate). */
  const [pendingPhotoRemoval, setPendingPhotoRemoval] = useState<EntityFormImageItem[] | null>(
    null,
  );

  const form = useZodForm(assetEditSchema, {
    asset_name: asset?.asset_name ?? "",
    class_code: asset?.class_code ?? "",
    family_code: asset?.family_code ?? null,
    subfamily_code: asset?.subfamily_code ?? null,
    criticality_code: asset?.criticality_code ?? "",
    status_code: asset?.status_code ?? "",
    manufacturer: asset?.manufacturer ?? null,
    model: asset?.model ?? null,
    serial_number: asset?.serial_number ?? null,
    maintainable_boundary: asset?.maintainable_boundary ?? true,
    org_node_id: asset?.org_node_id ?? (0 as unknown as number),
    commissioned_at: asset?.commissioned_at ?? null,
    decommissioned_at: asset?.decommissioned_at ?? null,
    rams_schedule_reference_value_id: asset?.rams_schedule_reference_value_id ?? null,
    rams_utilization_factor: asset?.rams_utilization_factor ?? 1.0,
  });

  const {
    register,
    handleSubmit,
    setValue,
    watch,
    reset,
    formState: { errors, isDirty },
  } = form;

  const loadPhotos = useCallback(async (assetId: number) => {
    setPhotosError(null);
    try {
      const photos = await listAssetPhotos(assetId);
      const items: EntityFormImageItem[] = await Promise.all(
        photos.map(async (photo, index) => {
          let previewUrl: string | null = null;
          try {
            const preview = await readAssetPhotoPreview(photo.id);
            previewUrl = photoDataUrl(preview);
          } catch {
            previewUrl = null;
          }
          return {
            id: String(photo.id),
            name: photo.file_name,
            previewUrl,
            isPrimary: index === 0,
          };
        }),
      );
      setPhotoItems(items);
    } catch (err) {
      setPhotoItems([]);
      setPhotosError(appErrorReason(err));
    }
  }, []);

  useEffect(() => {
    if (!open || !asset) return;
    setSubmitError(null);
    setOrgTreeError(null);
    setReferenceCatalogError(null);
    setCatalogReady(false);
    // Without the organisation view right the list is not requested; the field says why.
    if (canReadOrg)
      void listOrgTree()
        .then(setOrgNodes)
        .catch((err) => {
          console.error("[AssetEditForm] listOrgTree failed", err);
          setOrgTreeError(`${t("createForm.orgTreeLoadFailed")} ${appErrorReason(err)}`);
        });
    void getEquipmentTaxonomyCatalog()
      .then(() => setCatalogReady(true))
      .catch((err) => {
        console.error("[AssetEditForm] getEquipmentTaxonomyCatalog failed", err);
        setReferenceCatalogError(
          `${t("createForm.referenceCatalogLoadFailed")} ${appErrorReason(err)}`,
        );
        setCatalogReady(false);
      });
    void loadPhotos(asset.id);

    reset({
      asset_name: asset.asset_name,
      class_code: asset.class_code ?? "",
      family_code: asset.family_code ?? null,
      subfamily_code: asset.subfamily_code ?? null,
      criticality_code: asset.criticality_code ?? "",
      status_code: asset.status_code,
      manufacturer: asset.manufacturer ?? null,
      model: asset.model ?? null,
      serial_number: asset.serial_number ?? null,
      maintainable_boundary: asset.maintainable_boundary,
      org_node_id: asset.org_node_id ?? (0 as unknown as number),
      commissioned_at: asset.commissioned_at ?? null,
      decommissioned_at: asset.decommissioned_at ?? null,
      rams_schedule_reference_value_id: asset.rams_schedule_reference_value_id ?? null,
      rams_utilization_factor: asset.rams_utilization_factor ?? 1.0,
    });
  }, [open, asset, reset, t, loadPhotos, canReadOrg]);

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
      setValue("family_code", null, { shouldValidate: true, shouldDirty: true });
      setValue("subfamily_code", null, { shouldValidate: true, shouldDirty: true });
    }
  }, [watchedClassCode, setValue]);

  useEffect(() => {
    const prev = prevFamilyRef.current;
    const next = watchedFamilyCode ?? null;
    prevFamilyRef.current = next;
    if (prev != null && prev !== next) {
      setValue("subfamily_code", null, { shouldValidate: true, shouldDirty: true });
    }
  }, [watchedFamilyCode, setValue]);

  const handleCloseRequest = useCallback(() => {
    if (isDirty) {
      setShowUnsavedPrompt(true);
    } else {
      closeForm();
    }
  }, [closeForm, isDirty]);

  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (next) return;
      if (saving || photosBusy) return;
      if (isDirty) {
        setShowUnsavedPrompt(true);
        return;
      }
      closeForm();
    },
    [closeForm, isDirty, saving, photosBusy],
  );

  const handlePickPhotos = useCallback(async () => {
    if (!asset) return;
    const staged = await pickEntityFormImages();
    if (staged.length === 0) return;

    setPhotosBusy(true);
    try {
      for (const item of staged) {
        if (!item.path) continue;
        await uploadAssetPhoto({
          asset_id: asset.id,
          source_path: item.path,
          caption: null,
        });
      }
      await loadPhotos(asset.id);
      toast({ title: t("photos.uploaded") });
    } catch (err) {
      toast({ title: appErrorReason(err), variant: "destructive" });
    } finally {
      setPhotosBusy(false);
    }
    return [];
  }, [asset, loadPhotos, t, toast]);

  const handlePhotosChange = useCallback(
    (next: EntityFormImageItem[]) => {
      if (!asset) {
        setPhotoItems(next);
        return;
      }
      const removed = photoItems.filter((prev) => !next.some((n) => n.id === prev.id));
      if (removed.length === 0) {
        setPhotoItems(next);
        return;
      }
      // Deleting a stored photo is immediate (not undone by Cancel): ask first.
      setPendingPhotoRemoval(removed);
    },
    [asset, photoItems],
  );

  const deletePhotos = async (removed: EntityFormImageItem[]) => {
    if (!asset) return;
    setPhotosBusy(true);
    try {
      for (const item of removed) {
        const id = Number(item.id);
        if (!Number.isFinite(id)) continue;
        await deleteAssetPhoto(id);
      }
      toast({ title: t("photos.deleted") });
    } catch (err) {
      toast({ title: appErrorReason(err), variant: "destructive" });
    } finally {
      await loadPhotos(asset.id);
      setPhotosBusy(false);
    }
  };

  const onSubmit: Parameters<typeof handleSubmit>[0] = async (values) => {
    if (!asset) return;
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

    const payload: UpdateAssetIdentityPayload = {
      asset_name: values.asset_name ?? asset.asset_name,
      class_code: values.class_code ?? asset.class_code ?? "",
      criticality_code: values.criticality_code ?? asset.criticality_code ?? "",
      status_code: values.status_code ?? asset.status_code,
      maintainable_boundary: values.maintainable_boundary ?? true,
      ...(values.rams_schedule_reference_value_id !== undefined
        ? { rams_schedule_reference_value_id: values.rams_schedule_reference_value_id }
        : {}),
      ...(values.rams_utilization_factor !== undefined
        ? { rams_utilization_factor: values.rams_utilization_factor }
        : {}),
      ...(values.family_code !== undefined ? { family_code: values.family_code } : {}),
      ...(values.subfamily_code !== undefined ? { subfamily_code: values.subfamily_code } : {}),
      ...(values.manufacturer !== undefined ? { manufacturer: values.manufacturer } : {}),
      ...(values.model !== undefined ? { model: values.model } : {}),
      ...(values.serial_number !== undefined ? { serial_number: values.serial_number } : {}),
      ...(values.commissioned_at !== undefined ? { commissioned_at: values.commissioned_at } : {}),
      ...(values.decommissioned_at !== undefined
        ? { decommissioned_at: values.decommissioned_at }
        : {}),
    };

    try {
      const updated = await submitUpdate(asset.id, payload, asset.row_version);
      if (values.org_node_id && values.org_node_id !== asset.org_node_id) {
        await useAssetStore
          .getState()
          .moveAssetOrgNode(asset.id, values.org_node_id, updated.row_version);
      }
      closeForm();
    } catch (err) {
      setSubmitError(appErrorReason(err));
    }
  };

  const watchedOrgNode = watch("org_node_id");
  const busy = saving || photosBusy;

  if (!asset) {
    return null;
  }

  return (
    <>
      <EntityFormDialog
        open={open}
        onOpenChange={handleOpenChange}
        title={t("editForm.title")}
        description={t("editForm.description")}
        footer={
          <EntityFormFooter
            formId={FORM_ID}
            cancelLabel={t("decommission.cancel")}
            primaryLabel={
              <>
                {saving && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
                {t("editForm.submit")}
              </>
            }
            onCancel={() => handleCloseRequest()}
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

        <form id={FORM_ID} onSubmit={(e) => void handleSubmit(onSubmit)(e)} className="space-y-6">
          <EntityFormSection title={t("createForm.sections.identification")}>
            <FormField name="asset_code" label={t("form.identity.code.label")}>
              <Input
                id="asset_code"
                value={asset.asset_code}
                readOnly
                className="bg-surface-2 text-text-secondary"
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
                  onChange={(v) =>
                    setValue("class_code", v ?? "", { shouldValidate: true, shouldDirty: true })
                  }
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
                  onChange={(v) =>
                    setValue("family_code", v, { shouldValidate: true, shouldDirty: true })
                  }
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
                  onChange={(v) =>
                    setValue("subfamily_code", v, { shouldValidate: true, shouldDirty: true })
                  }
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
                onChange={(v) =>
                  setValue("criticality_code", v ?? "", {
                    shouldValidate: true,
                    shouldDirty: true,
                  })
                }
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
              {canReadOrg ? (
                <EquipmentSearchSelect
                  id="org_node_id"
                  options={orgOptions}
                  value={watchedOrgNode ? String(watchedOrgNode) : ""}
                  onChange={(v) =>
                    setValue("org_node_id", Number(v), { shouldValidate: true, shouldDirty: true })
                  }
                  placeholder={t("createForm.selectOrg")}
                  disabled={orgOptions.length === 0}
                  aria-invalid={!!errors.org_node_id}
                />
              ) : (
                <ListAccessNote list="orgNodes" />
              )}
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
                onCheckedChange={(c) =>
                  setValue("maintainable_boundary", c === true, { shouldDirty: true })
                }
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
                onChange={(v) =>
                  setValue("status_code", v ?? "", { shouldValidate: true, shouldDirty: true })
                }
                placeholder={t("createForm.selectStatus")}
                allowClear={false}
                disabled={assetRetired}
                excludeCodes={assetRetired ? [] : retiredStatusCodes}
                aria-invalid={!!errors.status_code}
              />
              <p className="mt-1 text-[11px] text-text-muted">
                {t(assetRetired ? "editForm.retiredHint" : "editForm.retireHint")}
              </p>
            </FormField>
            <FormField name="commissioned_at" label={t("detail.fields.commissioningDate")}>
              <Input id="commissioned_at" type="date" {...register("commissioned_at")} />
            </FormField>
            {asset.decommissioned_at ? (
              <FormField name="decommissioned_at" label={t("detail.fields.endOfLifeDate")}>
                <Input
                  id="decommissioned_at"
                  value={formatDate(asset.decommissioned_at)}
                  readOnly
                  className="bg-surface-2 text-text-secondary"
                />
              </FormField>
            ) : null}
          </EntityFormSection>

          <EntityFormSection title={t("createForm.sections.images")}>
            <EntityFormImageUploader
              items={photoItems}
              onChange={handlePhotosChange}
              onPickFiles={handlePickPhotos}
              disabled={busy}
              uploading={photosBusy}
              emptyLabel={photosError ?? t("editForm.imagesEmpty")}
              hintLabel={t("editForm.imagesHint")}
              primaryLabel={t("createForm.images.primary")}
              removeLabel={t("createForm.images.remove")}
              addLabel={t("createForm.images.add")}
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
                    { shouldValidate: true, shouldDirty: true },
                  )
                }
                placeholder={t("createForm.selectScheduleClass")}
                allowClear
              />
            </FormField>
            <FormField
              name="rams_utilization_factor"
              label={t("rams.utilization")}
              error={fieldError(errors.rams_utilization_factor?.message)}
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
                {t("createForm.fields.utilizationHint")}
              </p>
            </FormField>
          </EntityFormCollapsible>

          {asset.updated_at && (
            <p className="pt-1 text-xs text-text-muted">
              {t("editForm.lastModified", {
                date: formatDateTime(asset.updated_at),
              })}
            </p>
          )}

          {submitError && (
            <div
              role="alert"
              className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger"
            >
              {submitError}
            </div>
          )}
        </form>
      </EntityFormDialog>

      <ConfirmDialog
        open={pendingPhotoRemoval != null}
        onOpenChange={(o) => {
          if (!o) setPendingPhotoRemoval(null);
        }}
        title={t("editForm.photoDeleteTitle")}
        description={t("editForm.photoDeleteBody", { count: pendingPhotoRemoval?.length ?? 0 })}
        confirmLabel={t("editForm.photoDeleteConfirm")}
        destructive
        onConfirm={async () => {
          if (pendingPhotoRemoval) await deletePhotos(pendingPhotoRemoval);
        }}
      />

      <Dialog open={showUnsavedPrompt} onOpenChange={setShowUnsavedPrompt}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("editForm.unsavedTitle")}</DialogTitle>
            <DialogDescription>{t("editForm.unsavedDescription")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowUnsavedPrompt(false)}>
              {t("editForm.keepEditing")}
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setShowUnsavedPrompt(false);
                closeForm();
              }}
            >
              {t("editForm.discardChanges")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
