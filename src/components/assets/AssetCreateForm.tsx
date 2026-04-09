/**
 * AssetCreateForm.tsx
 *
 * Sheet dialog for creating a new asset. Uses useZodForm with
 * asset-create.schema for validation. Lookup selects load from
 * reference-service; parent/org comboboxes use debounced search.
 */

import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Textarea } from "@/components/ui/textarea";
import { useZodForm } from "@/lib/form-helpers";
import { assetCreateSchema } from "@/schemas/asset-create.schema";
import { listAssets } from "@/services/asset-service";
import { listOrgTree } from "@/services/org-node-service";
import { useAssetStore } from "@/stores/asset-store";
import type { Asset, CreateAssetPayload, OrgTreeRow } from "@shared/ipc-types";

// ── Criticality options ──────────────────────────────────────────────────────

const CRITICALITY_OPTIONS = [
  { value: "A", label: "A — Critical" },
  { value: "B", label: "B — Important" },
  { value: "C", label: "C — Standard" },
  { value: "D", label: "D — Low" },
];

const STATUS_OPTIONS = [
  { value: "ACTIVE", label: "Active" },
  { value: "OPERATIONAL", label: "Operational" },
  { value: "STANDBY", label: "Standby" },
];

export function AssetCreateForm() {
  const { t } = useTranslation("equipment");
  const open = useAssetStore((s) => s.showCreateForm);
  const closeForm = useAssetStore((s) => s.closeCreateForm);
  const submitCreate = useAssetStore((s) => s.createAsset);
  const saving = useAssetStore((s) => s.saving);
  const storeError = useAssetStore((s) => s.error);
  const parentPreFill = useAssetStore((s) => s.parentPreFill);

  const [submitError, setSubmitError] = useState<string | null>(null);

  // Lookup data
  const [orgNodes, setOrgNodes] = useState<OrgTreeRow[]>([]);
  const [assetOptions, setAssetOptions] = useState<Asset[]>([]);
  const [assetQuery, setAssetQuery] = useState("");
  const debounceRef = useRef<ReturnType<typeof setTimeout>>();

  const form = useZodForm(assetCreateSchema, {
    asset_code: "",
    asset_name: "",
    class_code: "",
    family_code: null,
    criticality_code: "",
    status_code: "ACTIVE",
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

  // Load org nodes on mount
  useEffect(() => {
    if (open) {
      void listOrgTree()
        .then(setOrgNodes)
        .catch(() => {});
    }
  }, [open]);

  // Pre-fill parent when opening with context
  useEffect(() => {
    if (open && parentPreFill) {
      setValue("parent_asset_id", parentPreFill.id);
    }
  }, [open, parentPreFill, setValue]);

  // Reset form when dialog opens
  useEffect(() => {
    if (open) {
      reset({
        asset_code: "",
        asset_name: "",
        class_code: "",
        family_code: null,
        criticality_code: "",
        status_code: "ACTIVE",
        manufacturer: null,
        model: null,
        serial_number: null,
        maintainable_boundary: true,
        org_node_id: 0 as unknown as number,
        parent_asset_id: parentPreFill?.id ?? null,
        commissioned_at: null,
        description: null,
      });
    }
  }, [open, parentPreFill, reset]);

  // Debounced parent asset search
  const searchParentAssets = useCallback((query: string) => {
    setAssetQuery(query);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => {
      if (query.length >= 2) {
        void listAssets(null, null, query)
          .then(setAssetOptions)
          .catch(() => {});
      } else {
        setAssetOptions([]);
      }
    }, 300);
  }, []);

  const onSubmit: Parameters<typeof handleSubmit>[0] = async (values) => {
    setSubmitError(null);
    const payload: CreateAssetPayload = {
      asset_code: values.asset_code,
      asset_name: values.asset_name,
      class_code: values.class_code,
      criticality_code: values.criticality_code,
      status_code: values.status_code ?? "ACTIVE",
      maintainable_boundary: values.maintainable_boundary ?? true,
      org_node_id: values.org_node_id,
      ...(values.family_code ? { family_code: values.family_code } : {}),
      ...(values.manufacturer ? { manufacturer: values.manufacturer } : {}),
      ...(values.model ? { model: values.model } : {}),
      ...(values.serial_number ? { serial_number: values.serial_number } : {}),
      ...(values.commissioned_at ? { commissioned_at: values.commissioned_at } : {}),
    };

    try {
      await submitCreate(payload);
      closeForm();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setSubmitError(msg);
    }
  };

  const watchedOrgNode = watch("org_node_id");
  const watchedParent = watch("parent_asset_id");

  return (
    <Sheet open={open} onOpenChange={(o) => !o && closeForm()}>
      <SheetContent className="w-[480px] overflow-auto sm:max-w-lg">
        <SheetHeader>
          <SheetTitle>{t("createForm.title")}</SheetTitle>
          <SheetDescription>{t("createForm.description")}</SheetDescription>
        </SheetHeader>

        <form onSubmit={(e) => void handleSubmit(onSubmit)(e)} className="space-y-4 py-4">
          {/* Code */}
          <FormField
            name="asset_code"
            label={t("form.identity.code.label")}
            error={errors.asset_code?.message}
            required
          >
            <Input
              id="asset_code"
              placeholder={t("form.identity.code.placeholder")}
              {...register("asset_code")}
            />
          </FormField>

          {/* Name */}
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

          {/* Class */}
          <FormField
            name="class_code"
            label={t("detail.fields.class")}
            error={errors.class_code?.message}
            required
          >
            <Input id="class_code" placeholder="e.g. PUMP, MOTOR" {...register("class_code")} />
          </FormField>

          {/* Family */}
          <FormField name="family_code" label={t("detail.fields.family")}>
            <Input id="family_code" placeholder="e.g. ROTATING" {...register("family_code")} />
          </FormField>

          {/* Criticality */}
          <FormField
            name="criticality_code"
            label={t("detail.fields.criticality")}
            error={errors.criticality_code?.message}
            required
          >
            <Select
              value={watch("criticality_code") ?? ""}
              onValueChange={(v) => setValue("criticality_code", v, { shouldValidate: true })}
            >
              <SelectTrigger id="criticality_code">
                <SelectValue placeholder={t("createForm.selectCriticality")} />
              </SelectTrigger>
              <SelectContent>
                {CRITICALITY_OPTIONS.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>

          {/* Status */}
          <FormField name="status_code" label={t("detail.fields.status")}>
            <Select
              value={watch("status_code") ?? ""}
              onValueChange={(v) => setValue("status_code", v, { shouldValidate: true })}
            >
              <SelectTrigger id="status_code">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {STATUS_OPTIONS.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {o.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>

          {/* Organization node */}
          <FormField
            name="org_node_id"
            label={t("detail.fields.site")}
            error={errors.org_node_id?.message}
            required
          >
            <Select
              value={watchedOrgNode ? String(watchedOrgNode) : ""}
              onValueChange={(v) => setValue("org_node_id", Number(v), { shouldValidate: true })}
            >
              <SelectTrigger id="org_node_id">
                <SelectValue placeholder={t("createForm.selectOrg")} />
              </SelectTrigger>
              <SelectContent>
                {orgNodes.map((n) => (
                  <SelectItem key={n.node.id} value={String(n.node.id)}>
                    {n.node.code} — {n.node.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>

          {/* Parent asset search */}
          <FormField name="parent_asset_id" label={t("createForm.parentAsset")}>
            <div className="space-y-1">
              <Input
                placeholder={t("createForm.searchParent")}
                value={assetQuery}
                onChange={(e) => searchParentAssets(e.target.value)}
              />
              {assetOptions.length > 0 && (
                <div className="max-h-32 overflow-auto rounded border border-surface-border bg-surface text-sm">
                  {assetOptions.map((a) => (
                    <button
                      key={a.id}
                      type="button"
                      className="w-full px-3 py-1.5 text-left hover:bg-muted text-xs"
                      onClick={() => {
                        setValue("parent_asset_id", a.id, { shouldValidate: true });
                        setAssetQuery(`${a.asset_code} — ${a.asset_name}`);
                        setAssetOptions([]);
                      }}
                    >
                      <span className="font-mono">{a.asset_code}</span>{" "}
                      <span className="text-text-muted">{a.asset_name}</span>
                    </button>
                  ))}
                </div>
              )}
              {watchedParent && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="text-xs"
                  onClick={() => {
                    setValue("parent_asset_id", null);
                    setAssetQuery("");
                  }}
                >
                  {t("createForm.clearParent")}
                </Button>
              )}
            </div>
          </FormField>

          {/* Manufacturer / Model / Serial */}
          <FormField name="manufacturer" label={t("detail.fields.manufacturer")}>
            <Input id="manufacturer" {...register("manufacturer")} />
          </FormField>
          <FormField name="model" label={t("detail.fields.model")}>
            <Input id="model" {...register("model")} />
          </FormField>
          <FormField name="serial_number" label={t("detail.fields.serialNumber")}>
            <Input id="serial_number" {...register("serial_number")} />
          </FormField>

          {/* Commissioning date */}
          <FormField name="commissioned_at" label={t("detail.fields.commissioningDate")}>
            <Input id="commissioned_at" type="date" {...register("commissioned_at")} />
          </FormField>

          {/* Description */}
          <FormField name="description" label={t("createForm.descriptionLabel")}>
            <Textarea id="description" rows={3} maxLength={2000} {...register("description")} />
          </FormField>

          {/* Maintainable boundary */}
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

          {/* ── Error banner ───────────────────────────────────────────── */}
          {(submitError || storeError) && (
            <div className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-status-danger">
              {submitError || storeError}
            </div>
          )}

          <SheetFooter className="pt-4">
            <Button type="button" variant="outline" onClick={closeForm}>
              {t("decommission.cancel")}
            </Button>
            <Button type="submit" disabled={saving}>
              {saving && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
              {t("createForm.submit")}
            </Button>
          </SheetFooter>
        </form>
      </SheetContent>
    </Sheet>
  );
}
