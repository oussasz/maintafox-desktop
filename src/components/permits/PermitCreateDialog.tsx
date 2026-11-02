import { useEffect, useMemo, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui";
import { DateTimeField } from "@/components/ui/date-time-field";
import { useAccessAt } from "@/hooks/use-access-at";
import { appErrorReason } from "@/lib/app-error";
import {
  createWorkPermit,
  type PermitTypeRecord,
  type WorkPermitRecord,
} from "@/services/permit-service";
import { listWos } from "@/services/wo-service";
import type { AssetSearchResult, WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { localInputToIso, toLocalInputValue } from "./permit-rules";
import { isSeededPermitTypeText, usePermitTypeName } from "./permit-type-name";

/** Open work orders offered for the chosen equipment (a picker, not a register). */
const WO_CHOICE_LIMIT = 50;
const NO_WORK_ORDER = "none";

export interface PermitCreateWorkOrder {
  id: number;
  code: string;
  assetLabel: string | null;
  /** The work order's site (`entity_id`), when it has one. */
  siteId?: number | null;
}

interface PermitCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  types: PermitTypeRecord[];
  /** When set, the permit is linked to this WO and its asset comes from the WO. */
  workOrder: PermitCreateWorkOrder | null;
  onCreated: (permit: WorkPermitRecord) => void;
}

/** Expiry proposed from the type's maximum duration; empty (to be entered) when it has none. */
function defaultExpiry(type: PermitTypeRecord | undefined): string {
  const hours = type?.max_duration_hours;
  if (hours == null || hours <= 0) return "";
  return toLocalInputValue(new Date(Date.now() + hours * 3_600_000));
}

export function PermitCreateDialog({
  open,
  onOpenChange,
  types,
  workOrder,
  onCreated,
}: PermitCreateDialogProps) {
  const { t } = useTranslation("permits");
  const typeName = usePermitTypeName();
  const [typeId, setTypeId] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  // L4-07: a permit raised from the permits screen can name the work order it covers.
  const [woChoice, setWoChoice] = useState(NO_WORK_ORDER);
  const [woOptions, setWoOptions] = useState<WorkOrder[]>([]);
  const [woError, setWoError] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedType = useMemo(() => types.find((ty) => String(ty.id) === typeId), [types, typeId]);

  useEffect(() => {
    if (!open) return;
    setTypeId("");
    setExpiresAt(defaultExpiry(undefined));
    setAsset(null);
    setWoChoice(NO_WORK_ORDER);
    setWoOptions([]);
    setWoError(null);
    setSubmitted(false);
    setError(null);
  }, [open]);

  const handleTypeChange = (value: string) => {
    setTypeId(value);
    setExpiresAt(defaultExpiry(types.find((ty) => String(ty.id) === value)));
  };

  const needsAsset = workOrder == null;
  const chosenWoId = woChoice === NO_WORK_ORDER ? null : Number(woChoice);
  // The permit is issued at the equipment's (or the work order's) site: once it is known, the
  // issue right is read there. Before, the page's "held anywhere" check let the dialog open and
  // the backend stays the authority.
  const siteId = workOrder != null ? (workOrder.siteId ?? null) : (asset?.org_node_id ?? null);
  const siteKnown = siteId != null && siteId > 0;
  const siteAccess = useAccessAt(siteKnown ? siteId : null);
  const rightsLoading = siteKnown && siteAccess.loading;
  const noIssueRightAtSite = siteKnown && !siteAccess.loading && !siteAccess.can(P.PTW_ISSUE);
  // Work orders are read with the work-order view right at the equipment's site.
  const canReadWos = siteKnown && !siteAccess.loading && siteAccess.can(P.OT_VIEW);

  // The open work orders of the chosen equipment: the server checks again that the work order is
  // on the permit's equipment.
  useEffect(() => {
    setWoChoice(NO_WORK_ORDER);
    setWoOptions([]);
    setWoError(null);
    if (!open || !needsAsset || !canReadWos || asset == null) return;
    let cancelled = false;
    listWos({ equipment_id: asset.id, open_only: true, limit: WO_CHOICE_LIMIT, offset: 0 })
      .then((page) => {
        if (!cancelled) setWoOptions(page.items);
      })
      .catch((err: unknown) => {
        if (!cancelled) setWoError(appErrorReason(err));
      });
    return () => {
      cancelled = true;
    };
  }, [open, needsAsset, canReadWos, asset]);
  const typeError = submitted && !typeId ? t("errors.required") : undefined;
  const assetError = submitted && needsAsset && !asset ? t("errors.required") : undefined;
  const expiryError = submitted && !localInputToIso(expiresAt) ? t("errors.required") : undefined;

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!typeId || (needsAsset && !asset) || !localInputToIso(expiresAt)) return;
    setBusy(true);
    setError(null);
    try {
      const created = await createWorkPermit({
        permit_type_id: Number(typeId),
        asset_id: asset?.id ?? null,
        entity_id: asset?.org_node_id ?? null,
        linked_work_order_id: workOrder?.id ?? chosenWoId,
        expires_at: localInputToIso(expiresAt),
      });
      onCreated(created);
      onOpenChange(false);
    } catch (err) {
      setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("create.title")}</DialogTitle>
          {workOrder ? (
            <DialogDescription>
              {t("create.workOrder")} : {workOrder.code}
            </DialogDescription>
          ) : null}
        </DialogHeader>
        <form className="space-y-4" onSubmit={(e) => void handleSubmit(e)}>
          <FormField name="permit-type" label={t("create.type")} required error={typeError}>
            <Select value={typeId} onValueChange={handleTypeChange}>
              <SelectTrigger id="permit-type" aria-label={t("create.type")}>
                <SelectValue placeholder={t("create.typePlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {types.map((ty) => (
                  <SelectItem key={ty.id} value={String(ty.id)}>
                    {typeName(ty.code, ty.name)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>

          <FormField
            name="permit-asset"
            label={t("create.asset")}
            required={needsAsset}
            error={assetError}
          >
            {workOrder ? (
              <p className="text-sm text-text-primary">
                {workOrder.assetLabel ?? t("detail.none")}
              </p>
            ) : (
              <AssetPicker id="permit-asset" value={asset} onChange={setAsset} error={assetError} />
            )}
          </FormField>

          {needsAsset && canReadWos ? (
            <FormField
              name="permit-work-order"
              label={t("create.workOrderPick")}
              description={asset ? t("create.workOrderHint") : t("create.workOrderChooseAsset")}
            >
              {woError ? (
                <p role="alert" className="text-sm text-tone-danger-fg">
                  {t("create.workOrderLoadError", { message: woError })}
                </p>
              ) : asset != null && woOptions.length === 0 ? (
                <p className="text-sm text-text-muted">{t("create.workOrderEmpty")}</p>
              ) : (
                <Select value={woChoice} onValueChange={setWoChoice} disabled={asset == null}>
                  <SelectTrigger id="permit-work-order" aria-label={t("create.workOrderPick")}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NO_WORK_ORDER}>{t("create.workOrderNone")}</SelectItem>
                    {woOptions.map((wo) => (
                      <SelectItem key={wo.id} value={String(wo.id)}>
                        {`${wo.code} — ${wo.title}`}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </FormField>
          ) : null}

          <FormField
            name="permit-expires"
            label={t("create.expiresAt")}
            description={t("create.expiresHint")}
            required
            error={expiryError}
          >
            <DateTimeField
              id="permit-expires"
              value={expiresAt}
              onChange={(v) => setExpiresAt(v)}
            />
          </FormField>

          {selectedType?.description &&
          !isSeededPermitTypeText(selectedType.code, selectedType.description) ? (
            <p className="text-xs text-text-muted">{selectedType.description}</p>
          ) : null}

          {noIssueRightAtSite ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {t("siteScope.noIssueRightAtSite")}
            </p>
          ) : null}

          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("actions.cancel")}
            </Button>
            <Button type="submit" disabled={busy || noIssueRightAtSite || rightsLoading}>
              {t("create.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
