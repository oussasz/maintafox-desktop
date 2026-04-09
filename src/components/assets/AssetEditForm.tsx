/**
 * AssetEditForm.tsx
 *
 * Sheet dialog for editing an existing asset. Code field is read-only.
 * Parent select excludes self and own descendants to prevent cycles.
 * Dirty tracking with unsaved-changes confirmation on close.
 */

import { Loader2 } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { useZodForm } from "@/lib/form-helpers";
import { assetEditSchema } from "@/schemas/asset-edit.schema";
import { listAssets, listAssetChildren } from "@/services/asset-service";
import { listOrgTree } from "@/services/org-node-service";
import { useAssetStore } from "@/stores/asset-store";
import type { Asset, OrgTreeRow, UpdateAssetIdentityPayload } from "@shared/ipc-types";

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
  { value: "MAINTENANCE", label: "Maintenance" },
];

export function AssetEditForm() {
  const { t } = useTranslation("equipment");
  const open = useAssetStore((s) => s.showEditForm);
  const asset = useAssetStore((s) => s.editingAsset);
  const closeForm = useAssetStore((s) => s.closeEditForm);
  const submitUpdate = useAssetStore((s) => s.updateAsset);
  const saving = useAssetStore((s) => s.saving);

  const [orgNodes, setOrgNodes] = useState<OrgTreeRow[]>([]);
  const [assetOptions, setAssetOptions] = useState<Asset[]>([]);
  const [assetQuery, setAssetQuery] = useState("");
  const [descendantIds, setDescendantIds] = useState<Set<number>>(new Set());
  const [showUnsavedPrompt, setShowUnsavedPrompt] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout>>();

  const form = useZodForm(assetEditSchema, {
    asset_name: asset?.asset_name ?? "",
    class_code: asset?.class_code ?? "",
    family_code: asset?.family_code ?? null,
    criticality_code: asset?.criticality_code ?? "",
    status_code: asset?.status_code ?? "ACTIVE",
    manufacturer: asset?.manufacturer ?? null,
    model: asset?.model ?? null,
    serial_number: asset?.serial_number ?? null,
    maintainable_boundary: asset?.maintainable_boundary ?? true,
    org_node_id: asset?.org_node_id ?? (0 as unknown as number),
    parent_asset_id: null,
    commissioned_at: asset?.commissioned_at ?? null,
    decommissioned_at: asset?.decommissioned_at ?? null,
    description: null,
  });

  const {
    register,
    handleSubmit,
    setValue,
    watch,
    reset,
    formState: { errors, isDirty },
  } = form;

  // Load org nodes and descendants
  useEffect(() => {
    if (open && asset) {
      void listOrgTree()
        .then(setOrgNodes)
        .catch(() => {});
      // Load descendants to prevent cycle
      void listAssetChildren(asset.id)
        .then((children) => {
          const ids = new Set(children.map((c) => c.child_asset_id));
          setDescendantIds(ids);
        })
        .catch(() => {});
      // Reset the form with asset data
      reset({
        asset_name: asset.asset_name,
        class_code: asset.class_code ?? "",
        family_code: asset.family_code ?? null,
        criticality_code: asset.criticality_code ?? "",
        status_code: asset.status_code,
        manufacturer: asset.manufacturer ?? null,
        model: asset.model ?? null,
        serial_number: asset.serial_number ?? null,
        maintainable_boundary: asset.maintainable_boundary,
        org_node_id: asset.org_node_id ?? (0 as unknown as number),
        parent_asset_id: null,
        commissioned_at: asset.commissioned_at ?? null,
        decommissioned_at: asset.decommissioned_at ?? null,
        description: null,
      });
      setAssetQuery("");
    }
  }, [open, asset, reset]);

  // Debounced parent asset search (exclude self + descendants)
  const searchParentAssets = useCallback(
    (query: string) => {
      setAssetQuery(query);
      if (debounceRef.current) clearTimeout(debounceRef.current);
      debounceRef.current = setTimeout(() => {
        if (query.length >= 2) {
          void listAssets(null, null, query)
            .then((results) => {
              const filtered = results.filter(
                (a) => a.id !== asset?.id && !descendantIds.has(a.id),
              );
              setAssetOptions(filtered);
            })
            .catch(() => {});
        } else {
          setAssetOptions([]);
        }
      }, 300);
    },
    [asset?.id, descendantIds],
  );

  const handleClose = () => {
    if (isDirty) {
      setShowUnsavedPrompt(true);
    } else {
      closeForm();
    }
  };

  const onSubmit: Parameters<typeof handleSubmit>[0] = async (values) => {
    if (!asset) return;
    const payload: UpdateAssetIdentityPayload = {
      asset_name: values.asset_name ?? asset.asset_name,
      class_code: values.class_code ?? asset.class_code ?? "",
      criticality_code: values.criticality_code ?? asset.criticality_code ?? "",
      status_code: values.status_code ?? asset.status_code,
      maintainable_boundary: values.maintainable_boundary ?? true,
      ...(values.family_code !== undefined ? { family_code: values.family_code } : {}),
      ...(values.manufacturer !== undefined ? { manufacturer: values.manufacturer } : {}),
      ...(values.model !== undefined ? { model: values.model } : {}),
      ...(values.serial_number !== undefined ? { serial_number: values.serial_number } : {}),
      ...(values.commissioned_at !== undefined ? { commissioned_at: values.commissioned_at } : {}),
      ...(values.decommissioned_at !== undefined
        ? { decommissioned_at: values.decommissioned_at }
        : {}),
    };

    await submitUpdate(asset.id, payload, asset.row_version);
    closeForm();
  };

  const watchedOrgNode = watch("org_node_id");
  const watchedParent = watch("parent_asset_id");

  return (
    <>
      <Sheet open={open} onOpenChange={(o) => !o && handleClose()}>
        <SheetContent className="w-[480px] overflow-auto sm:max-w-lg">
          <SheetHeader>
            <SheetTitle>{t("editForm.title")}</SheetTitle>
            <SheetDescription>{t("editForm.description")}</SheetDescription>
          </SheetHeader>

          <form onSubmit={(e) => void handleSubmit(onSubmit)(e)} className="space-y-4 py-4">
            {/* Code — read-only */}
            <FormField name="asset_code" label={t("form.identity.code.label")}>
              <Input
                id="asset_code"
                value={asset?.asset_code ?? ""}
                readOnly
                className="bg-muted"
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
              <Input id="class_code" {...register("class_code")} />
            </FormField>

            {/* Family */}
            <FormField name="family_code" label={t("detail.fields.family")}>
              <Input id="family_code" {...register("family_code")} />
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
                  <SelectValue />
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
                  <SelectValue />
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

            {/* Last modified info */}
            {asset?.updated_at && (
              <p className="text-xs text-text-muted pt-2">
                {t("editForm.lastModified", {
                  date: new Date(asset.updated_at).toLocaleString(),
                })}
              </p>
            )}

            <SheetFooter className="pt-4">
              <Button type="button" variant="outline" onClick={handleClose}>
                {t("decommission.cancel")}
              </Button>
              <Button type="submit" disabled={saving}>
                {saving && <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />}
                {t("editForm.submit")}
              </Button>
            </SheetFooter>
          </form>
        </SheetContent>
      </Sheet>

      {/* Unsaved changes confirmation */}
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
