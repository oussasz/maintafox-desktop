import { Loader2, Search } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

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
import { searchAssets } from "@/services/asset-search-service";
import {
  createWorkPermit,
  type PermitTypeRecord,
  type WorkPermitRecord,
} from "@/services/permit-service";
import { toErrorMessage } from "@/utils/errors";
import type { AssetSearchResult } from "@shared/ipc-types";

import { localInputToIso, toLocalInputValue } from "./permit-rules";

export interface PermitCreateWorkOrder {
  id: number;
  code: string;
  assetLabel: string | null;
}

interface PermitCreateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  types: PermitTypeRecord[];
  /** When set, the permit is linked to this WO and its asset comes from the WO. */
  workOrder: PermitCreateWorkOrder | null;
  onCreated: (permit: WorkPermitRecord) => void;
}

function defaultExpiry(type: PermitTypeRecord | undefined): string {
  const hours = type?.max_duration_hours ?? 8;
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
  const [assetQuery, setAssetQuery] = useState("");
  const [assetResults, setAssetResults] = useState<AssetSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const selectedType = useMemo(() => types.find((ty) => String(ty.id) === typeId), [types, typeId]);

  useEffect(() => {
    if (!open) return;
    setTypeId("");
    setExpiresAt(defaultExpiry(undefined));
    setAsset(null);
    setAssetQuery("");
    setAssetResults([]);
    setSubmitted(false);
    setError(null);
  }, [open]);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const handleTypeChange = (value: string) => {
    setTypeId(value);
    setExpiresAt(defaultExpiry(types.find((ty) => String(ty.id) === value)));
  };

  const handleAssetQuery = (query: string) => {
    setAssetQuery(query);
    if (timer.current) clearTimeout(timer.current);
    if (!query.trim()) {
      setAssetResults([]);
      return;
    }
    timer.current = setTimeout(() => {
      setSearching(true);
      setSearchError(null);
      searchAssets({ query, limit: 20, include_decommissioned: false })
        .then(setAssetResults)
        .catch((err: unknown) => {
          setAssetResults([]);
          setSearchError(toErrorMessage(err));
        })
        .finally(() => setSearching(false));
    }, 250);
  };

  const needsAsset = workOrder == null;
  const typeError = submitted && !typeId ? t("errors.required") : undefined;
  const assetError = submitted && needsAsset && !asset ? t("errors.required") : undefined;

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (!typeId || (needsAsset && !asset)) return;
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
      setError(toErrorMessage(err));
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
            ) : asset ? (
              <div className="flex items-center justify-between rounded-md border border-surface-border p-2 text-sm">
                <span>
                  <span className="font-mono text-xs text-text-muted">{asset.asset_code}</span>{" "}
                  {asset.asset_name}
                </span>
                <Button type="button" variant="ghost" size="sm" onClick={() => setAsset(null)}>
                  {t("create.assetChange")}
                </Button>
              </div>
            ) : (
              <div className="space-y-1">
                <div className="relative">
                  <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-text-muted" />
                  <Input
                    id="permit-asset"
                    className="pl-9"
                    placeholder={t("create.assetSearch")}
                    value={assetQuery}
                    onChange={(e) => handleAssetQuery(e.target.value)}
                  />
                  {searching ? (
                    <Loader2 className="absolute right-2.5 top-2.5 h-4 w-4 animate-spin text-text-muted" />
                  ) : null}
                </div>
                {searchError ? (
                  <p role="alert" className="text-xs text-status-danger">
                    {t("create.assetSearchError", { message: searchError })}
                  </p>
                ) : null}
                {assetResults.length > 0 ? (
                  <ul className="max-h-48 overflow-y-auto rounded-md border border-surface-border">
                    {assetResults.map((a) => (
                      <li key={a.id}>
                        <button
                          type="button"
                          className="flex w-full gap-2 px-3 py-1.5 text-left text-sm hover:bg-surface-1"
                          onClick={() => {
                            setAsset(a);
                            setAssetResults([]);
                          }}
                        >
                          <span className="font-mono text-xs text-text-muted">{a.asset_code}</span>
                          <span className="truncate">{a.asset_name}</span>
                          {a.org_node_name ? (
                            <span className="ml-auto truncate text-xs text-text-muted">
                              {a.org_node_name}
                            </span>
                          ) : null}
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : null}
              </div>
            )}
          </FormField>

          <FormField
            name="permit-expires"
            label={t("create.expiresAt")}
            description={t("create.expiresHint")}
            required
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

          {error ? (
            <p role="alert" className="text-sm text-status-danger">
              {error}
            </p>
          ) : null}

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("actions.cancel")}
            </Button>
            <Button type="submit" disabled={busy}>
              {t("create.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
