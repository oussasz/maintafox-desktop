/**
 * AssetCreateForm.tsx
 *
 * Centered modal (DI/OT pattern) for creating an asset. Reference fields use
 * searchable selects; parent equipment links after create via hierarchy API.
 */

import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { EquipmentParentPicker } from "@/components/assets/EquipmentParentPicker";
import {
  EquipmentSearchSelect,
  type EquipmentSearchOption,
} from "@/components/assets/EquipmentSearchSelect";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { Textarea } from "@/components/ui/textarea";
import { useZodForm } from "@/lib/form-helpers";
import { assetCreateSchema } from "@/schemas/asset-create.schema";
import { getEquipmentTaxonomyCatalog } from "@/services/asset-service";
import { listOrgTree } from "@/services/org-node-service";
import { useAssetStore } from "@/stores/asset-store";
import type { CreateAssetPayload, EquipmentTaxonomyCatalog, OrgTreeRow } from "@shared/ipc-types";

function taxonomyToOptions(
  catalog: EquipmentTaxonomyCatalog | null,
  key: "classes" | "families" | "subfamilies" | "criticalities" | "statuses",
): EquipmentSearchOption[] {
  if (!catalog) return [];
  return catalog[key].map((o) => ({
    value: o.code,
    label: o.label,
    description: o.code,
  }));
}

export function AssetCreateForm() {
  const { t } = useTranslation("equipment");
  const open = useAssetStore((s) => s.showCreateForm);
  const closeForm = useAssetStore((s) => s.closeCreateForm);
  const submitCreate = useAssetStore((s) => s.createAsset);
  const saving = useAssetStore((s) => s.saving);
  const storeError = useAssetStore((s) => s.error);
  const parentPreFill = useAssetStore((s) => s.parentPreFill);

  const [submitError, setSubmitError] = useState<string | null>(null);
  const [taxonomy, setTaxonomy] = useState<EquipmentTaxonomyCatalog | null>(null);
  const [orgNodes, setOrgNodes] = useState<OrgTreeRow[]>([]);
  const [catalogError, setCatalogError] = useState<string | null>(null);

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
    setCatalogError(null);
    void listOrgTree()
      .then(setOrgNodes)
      .catch(() => setCatalogError(t("createForm.catalogLoadFailed")));
    void getEquipmentTaxonomyCatalog()
      .then(setTaxonomy)
      .catch(() => setCatalogError(t("createForm.catalogLoadFailed")));
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
      description: null,
    });
  }, [open, parentPreFill, reset]);

  const classOptions = useMemo(() => taxonomyToOptions(taxonomy, "classes"), [taxonomy]);
  const allFamilyOptions = useMemo(() => taxonomyToOptions(taxonomy, "families"), [taxonomy]);
  const allSubfamilyOptions = useMemo(() => taxonomyToOptions(taxonomy, "subfamilies"), [taxonomy]);
  const criticalityOptions = useMemo(
    () => taxonomyToOptions(taxonomy, "criticalities"),
    [taxonomy],
  );
  const statusOptions = useMemo(() => taxonomyToOptions(taxonomy, "statuses"), [taxonomy]);
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
  const watchedSubfamilyCode = watch("subfamily_code");

  const selectedClassId = useMemo(() => {
    if (!taxonomy || !watchedClassCode) return null;
    return taxonomy.classes.find((c) => c.code === watchedClassCode)?.id ?? null;
  }, [taxonomy, watchedClassCode]);

  const selectedFamilyId = useMemo(() => {
    if (!taxonomy || !watchedFamilyCode) return null;
    return taxonomy.families.find((f) => f.code === watchedFamilyCode)?.id ?? null;
  }, [taxonomy, watchedFamilyCode]);

  const familyOptions = useMemo(() => {
    if (selectedClassId == null || !taxonomy) return [];
    return taxonomy.families
      .filter((f) => f.parent_id === selectedClassId)
      .map((o) => ({ value: o.code, label: o.label, description: o.code }));
  }, [taxonomy, selectedClassId]);

  const subfamilyOptions = useMemo(() => {
    if (selectedFamilyId == null || !taxonomy) return [];
    return taxonomy.subfamilies
      .filter((s) => s.parent_id === selectedFamilyId)
      .map((o) => ({ value: o.code, label: o.label, description: o.code }));
  }, [taxonomy, selectedFamilyId]);

  useEffect(() => {
    if (!watchedClassCode || !watchedFamilyCode || !taxonomy) return;
    const fam = taxonomy.families.find((f) => f.code === watchedFamilyCode);
    if (!fam || fam.parent_id !== selectedClassId) {
      setValue("family_code", null, { shouldValidate: true });
      setValue("subfamily_code", null, { shouldValidate: true });
    }
  }, [watchedClassCode, watchedFamilyCode, taxonomy, selectedClassId, setValue]);

  useEffect(() => {
    if (!watchedSubfamilyCode || !watchedFamilyCode || !taxonomy) return;
    const sf = taxonomy.subfamilies.find((s) => s.code === watchedSubfamilyCode);
    if (!sf || sf.parent_id !== selectedFamilyId) {
      setValue("subfamily_code", null, { shouldValidate: true });
    }
  }, [watchedSubfamilyCode, watchedFamilyCode, selectedFamilyId, taxonomy, setValue]);

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
    };

    try {
      await submitCreate(
        payload,
        values.parent_asset_id != null ? { parentAssetId: values.parent_asset_id } : {},
      );
      closeForm();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setSubmitError(msg);
    }
  };

  const handleOpenChange = useCallback(
    (next: boolean) => {
      if (!next && !saving) closeForm();
    },
    [closeForm, saving],
  );

  const watchedOrgNode = watch("org_node_id");

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        className="max-w-2xl max-h-[90vh] flex flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader className="shrink-0 space-y-1.5 px-6 pt-6 pb-2">
          <DialogTitle className="text-lg font-semibold tracking-tight">
            {t("createForm.title")}
          </DialogTitle>
          <DialogDescription className="text-sm text-text-muted">
            {t("createForm.description")}
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-2">
          {catalogError && (
            <div className="mb-3 rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-sm text-status-warning">
              {catalogError}
            </div>
          )}

          <form
            id="asset-create-form"
            onSubmit={(e) => void handleSubmit(onSubmit)(e)}
            className="space-y-4"
          >
            <FormField
              name="asset_code"
              label={t("form.identity.code.label")}
              error={errors.asset_code?.message}
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
              error={errors.asset_name?.message}
              required
            >
              <Input
                id="asset_name"
                placeholder={t("form.identity.name.placeholder")}
                {...register("asset_name")}
              />
            </FormField>

            <FormField
              name="class_code"
              label={t("detail.fields.class")}
              error={errors.class_code?.message}
              required
            >
              <EquipmentSearchSelect
                id="class_code"
                options={classOptions}
                value={watch("class_code") ?? ""}
                onChange={(v) => setValue("class_code", v, { shouldValidate: true })}
                placeholder={t("createForm.selectClass")}
                disabled={!taxonomy || classOptions.length === 0}
                aria-invalid={!!errors.class_code}
              />
            </FormField>

            <FormField name="family_code" label={t("detail.fields.family")}>
              <EquipmentSearchSelect
                id="family_code"
                options={[
                  { value: "__none__", label: t("createForm.familyNone"), description: "" },
                  ...(selectedClassId == null ? allFamilyOptions : familyOptions),
                ]}
                value={watch("family_code") ? (watch("family_code") as string) : "__none__"}
                onChange={(v) =>
                  setValue("family_code", v === "__none__" ? null : v, { shouldValidate: true })
                }
                placeholder={t("createForm.selectFamilyOptional")}
                disabled={!taxonomy || selectedClassId == null}
              />
            </FormField>

            <FormField name="subfamily_code" label={t("detail.fields.subfamily")}>
              <EquipmentSearchSelect
                id="subfamily_code"
                options={[
                  { value: "__none__", label: t("createForm.familyNone"), description: "" },
                  ...(selectedFamilyId == null ? allSubfamilyOptions : subfamilyOptions),
                ]}
                value={watch("subfamily_code") ? (watch("subfamily_code") as string) : "__none__"}
                onChange={(v) =>
                  setValue("subfamily_code", v === "__none__" ? null : v, { shouldValidate: true })
                }
                placeholder={t("createForm.selectSubfamilyOptional")}
                disabled={!taxonomy || selectedFamilyId == null}
              />
            </FormField>

            <FormField
              name="criticality_code"
              label={t("detail.fields.criticality")}
              error={errors.criticality_code?.message}
              required
            >
              <EquipmentSearchSelect
                id="criticality_code"
                options={criticalityOptions}
                value={watch("criticality_code") ?? ""}
                onChange={(v) => setValue("criticality_code", v, { shouldValidate: true })}
                placeholder={t("createForm.selectCriticality")}
                disabled={!taxonomy || criticalityOptions.length === 0}
                aria-invalid={!!errors.criticality_code}
              />
            </FormField>

            <FormField
              name="status_code"
              label={t("detail.fields.status")}
              error={errors.status_code?.message}
              required
            >
              <EquipmentSearchSelect
                id="status_code"
                options={statusOptions}
                value={watch("status_code") ?? ""}
                onChange={(v) => setValue("status_code", v, { shouldValidate: true })}
                placeholder={t("createForm.selectStatus")}
                disabled={!taxonomy || statusOptions.length === 0}
                aria-invalid={!!errors.status_code}
              />
            </FormField>

            <FormField
              name="org_node_id"
              label={t("detail.fields.site")}
              error={errors.org_node_id?.message}
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

            <FormField name="manufacturer" label={t("detail.fields.manufacturer")}>
              <Input id="manufacturer" {...register("manufacturer")} />
            </FormField>
            <FormField name="model" label={t("detail.fields.model")}>
              <Input id="model" {...register("model")} />
            </FormField>
            <FormField name="serial_number" label={t("detail.fields.serialNumber")}>
              <Input id="serial_number" {...register("serial_number")} />
            </FormField>

            <FormField name="commissioned_at" label={t("detail.fields.commissioningDate")}>
              <Input id="commissioned_at" type="date" {...register("commissioned_at")} />
            </FormField>

            <FormField name="description" label={t("createForm.descriptionLabel")}>
              <Textarea id="description" rows={3} maxLength={2000} {...register("description")} />
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

            {(submitError || storeError) && (
              <div className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
                {submitError || storeError}
              </div>
            )}
          </form>
        </div>

        <DialogFooter className="shrink-0 border-t border-surface-border bg-surface-0 px-6 py-4">
          <Button type="button" variant="outline" onClick={() => closeForm()} disabled={saving}>
            {t("decommission.cancel")}
          </Button>
          <Button type="submit" form="asset-create-form" disabled={saving || !taxonomy}>
            {saving && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
            {t("createForm.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
