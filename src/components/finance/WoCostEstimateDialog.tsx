/**
 * OT estimate by cost category (SAP PM "estimated costs") and the optional cost-centre override
 * the OT settles to. Amounts are in the tenant currency.
 */
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { COST_BUCKETS, type CostBucket } from "@/components/finance/finance-tone";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { Button } from "@/components/ui/button";
import { ErrorState } from "@/components/ui/data-state";
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
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { listCostCenters } from "@/services/budget-service";
import { type WoCostEstimate, upsertWoCostEstimate } from "@/services/finance-cost-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { CostCenter } from "@shared/ipc-types";

const AUTO = "__auto__";

/** Parses an amount typed by the user (comma or dot decimals); null when invalid or negative. */
export function parseAmount(raw: string): number | null {
  const normalized = raw.replace(/\s/g, "").replace(",", ".");
  if (normalized === "") return 0;
  const value = Number(normalized);
  return Number.isFinite(value) && value >= 0 ? value : null;
}

export interface WoCostEstimateDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workOrderId: number;
  workOrderCode: string;
  estimate: WoCostEstimate | null;
  /** Cost centre resolved from the asset / organisation, shown for the "automatic" choice. */
  resolvedCostCenterLabel: string | null;
  /** Cost centre the OT settles to today (override or resolved), shown when it cannot be changed. */
  currentCostCenterLabel: string | null;
  /** Choosing the cost centre is a posting decision (`fin.post`); others see it read-only. */
  canChangeCostCenter: boolean;
  onSaved: () => void;
}

export function WoCostEstimateDialog({
  open,
  onOpenChange,
  workOrderId,
  workOrderCode,
  estimate,
  resolvedCostCenterLabel,
  currentCostCenterLabel,
  canChangeCostCenter,
  onSaved,
}: WoCostEstimateDialogProps) {
  const { t } = useTranslation("budget");
  const { formatCurrency, currencyCode } = useFormatters();
  const { categoryLabel } = useFinanceCatalogs();
  const [amounts, setAmounts] = useState<Record<CostBucket, string>>(() => ({
    labor: estimate ? String(estimate.estimated_labor) : "",
    parts: estimate ? String(estimate.estimated_parts) : "",
    services: estimate ? String(estimate.estimated_services) : "",
    other: estimate ? String(estimate.estimated_other) : "",
  }));
  const [costCenter, setCostCenter] = useState<string>(
    estimate?.cost_center_id != null ? String(estimate.cost_center_id) : AUTO,
  );
  const [notes, setNotes] = useState(estimate?.notes ?? "");
  const [centers, setCenters] = useState<CostCenter[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    if (!open || !canChangeCostCenter) return;
    listCostCenters({ include_inactive: false })
      .then(setCenters)
      .catch((err: unknown) => setError(err));
  }, [open, canChangeCostCenter]);

  const parsed = useMemo(
    () =>
      Object.fromEntries(COST_BUCKETS.map((b) => [b, parseAmount(amounts[b])])) as Record<
        CostBucket,
        number | null
      >,
    [amounts],
  );
  const invalid = COST_BUCKETS.some((b) => parsed[b] == null);
  const total = COST_BUCKETS.reduce((sum, b) => sum + (parsed[b] ?? 0), 0);

  const submit = async () => {
    if (invalid || saving) return;
    setSaving(true);
    setError(null);
    try {
      await upsertWoCostEstimate({
        work_order_id: workOrderId,
        cost_center_id: costCenter === AUTO ? null : Number(costCenter),
        estimated_labor: parsed.labor ?? 0,
        estimated_parts: parsed.parts ?? 0,
        estimated_services: parsed.services ?? 0,
        estimated_other: parsed.other ?? 0,
        notes: notes.trim() || null,
        expected_row_version: estimate?.row_version ?? null,
      });
      pushAppToast({ title: t("estimate.saved", { code: workOrderCode }), variant: "success" });
      onSaved();
      onOpenChange(false);
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("estimate.title", { code: workOrderCode })}</DialogTitle>
          <DialogDescription>
            {t("estimate.description", { currency: currencyCode })}
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="grid grid-cols-2 gap-3">
            {COST_BUCKETS.map((bucket) => (
              <div key={bucket} className="space-y-1">
                <Label htmlFor={`est-${bucket}`}>{categoryLabel(bucket)}</Label>
                <Input
                  id={`est-${bucket}`}
                  inputMode="decimal"
                  value={amounts[bucket]}
                  aria-invalid={parsed[bucket] == null}
                  onChange={(e) => setAmounts((a) => ({ ...a, [bucket]: e.target.value }))}
                  placeholder="0"
                />
                {parsed[bucket] == null ? (
                  <p className="text-xs text-tone-danger-fg">{t("estimate.invalidAmount")}</p>
                ) : null}
              </div>
            ))}
          </div>
          <p className="text-sm text-text-secondary">
            {t("estimate.total", { amount: formatCurrency(total) })}
          </p>
          <div className="space-y-1">
            <Label htmlFor="est-cc">{t("estimate.costCenter")}</Label>
            {canChangeCostCenter ? (
              <Select value={costCenter} onValueChange={setCostCenter}>
                <SelectTrigger id="est-cc">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={AUTO}>
                    {resolvedCostCenterLabel
                      ? t("estimate.ccAutoResolved", { cc: resolvedCostCenterLabel })
                      : t("estimate.ccAutoNone")}
                  </SelectItem>
                  {centers.map((c) => (
                    <SelectItem key={c.id} value={String(c.id)}>
                      {c.code} — {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <p id="est-cc" className="text-sm text-text-primary">
                {currentCostCenterLabel ?? t("estimate.ccAutoNone")}
              </p>
            )}
            <p className="text-xs text-text-muted">
              {canChangeCostCenter ? t("estimate.ccHint") : t("estimate.ccReadOnly")}
            </p>
          </div>
          <div className="space-y-1">
            <Label htmlFor="est-notes">{t("estimate.notes")}</Label>
            <Textarea
              id="est-notes"
              rows={2}
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
            />
          </div>
          {error ? <ErrorState error={error} title={t("estimate.saveError")} /> : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("actions.cancel")}
            </Button>
            <Button type="submit" disabled={invalid || saving}>
              {saving ? t("actions.saving") : t("actions.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
