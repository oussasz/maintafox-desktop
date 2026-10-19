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
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui";
import { useAccessAt } from "@/hooks/use-access-at";
import { appErrorReason } from "@/lib/app-error";
import {
  createWorkPermit,
  type PermitTypeRecord,
  type WorkPermitRecord,
} from "@/services/permit-service";
import type { AssetSearchResult } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { localInputToIso, toLocalInputValue } from "./permit-rules";

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
  const [typeId, setTypeId] = useState("");
  const [expiresAt, setExpiresAt] = useState("");
  const [asset, setAsset] = useState<AssetSearchResult | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedType = useMemo(() => types.find((ty) => String(ty.id) === typeId), [types, typeId]);

  useEffect(() => {
    if (!open) return;
    setTypeId("");
    setExpiresAt(defaultExpiry(undefined));
    setAsset(null);
    setSubmitted(false);
    setError(null);
  }, [open]);

  const handleTypeChange = (value: string) => {
    setTypeId(value);
    setExpiresAt(defaultExpiry(types.find((ty) => String(ty.id) === value)));
  };

  const needsAsset = workOrder == null;
  // The permit is issued at the equipment's (or the work order's) site: once it is known, the
  // issue right is read there. Before, the page's "held anywhere" check let the dialog open and
  // the backend stays the authority.
  const siteId = workOrder != null ? (workOrder.siteId ?? null) : (asset?.org_node_id ?? null);
  const siteKnown = siteId != null && siteId > 0;
  const siteAccess = useAccessAt(siteKnown ? siteId : null);
  const noIssueRightAtSite = siteKnown && !siteAccess.loading && !siteAccess.can(P.PTW_ISSUE);
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
        linked_work_order_id: workOrder?.id ?? null,
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
                    {ty.name}
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

          <FormField
            name="permit-expires"
            label={t("create.expiresAt")}
            description={t("create.expiresHint")}
            required
            error={expiryError}
          >
            <Input
              id="permit-expires"
              type="datetime-local"
              value={expiresAt}
              onChange={(e) => setExpiresAt(e.target.value)}
            />
          </FormField>

          {selectedType?.description ? (
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
            <Button type="submit" disabled={busy || noIssueRightAtSite}>
              {t("create.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
