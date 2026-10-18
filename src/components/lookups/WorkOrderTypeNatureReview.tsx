/**
 * Work-order types without a maintenance nature (a plant's own type carried over from the old
 * list): they count in no planned / reactive / emergency share until they get one. Listed with
 * how many work orders use them; holders of the reference-data manage right give each its nature
 * (picked from the maintenance natures, nothing preselected) with a reason, recorded server side.
 * Shown only when there is something to review.
 */

import { type FormEvent, useCallback, useEffect, useState } from "react";
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
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { ErrorState } from "@/components/ui/data-state";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorReason } from "@/lib/app-error";
import { listWorkOrderTypesToReview, setWorkOrderTypeNature } from "@/services/wo-service";
import { refreshWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import type { MaintenanceNatureCode, WorkOrderTypeToReview } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Shortest reason the server accepts (`MIN_NATURE_REASON_CHARS`). */
export const NATURE_REASON_MIN = 10;

/** The maintenance natures (EN 13306), in the order the program lists them. */
export const MAINTENANCE_NATURES: MaintenanceNatureCode[] = [
  "corrective_immediate",
  "corrective_deferred",
  "preventive_predetermined",
  "preventive_condition_based",
  "preventive_predictive",
  "improvement",
];

export function WorkOrderTypeNatureReview() {
  const { t } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const [rows, setRows] = useState<WorkOrderTypeToReview[] | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [target, setTarget] = useState<WorkOrderTypeToReview | null>(null);
  const [nature, setNature] = useState<string>("");
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setRows(await listWorkOrderTypesToReview());
    } catch (e) {
      setLoadError(e);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const open = (row: WorkOrderTypeToReview) => {
    setTarget(row);
    setNature("");
    setReason("");
    setSubmitted(false);
    setSaveError(null);
  };

  const reasonTooShort = reason.trim().length < NATURE_REASON_MIN;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!target) return;
    setSubmitted(true);
    setSaveError(null);
    if (!nature || reasonTooShort) return;
    setBusy(true);
    try {
      await setWorkOrderTypeNature({
        type_id: target.id,
        nature: nature as MaintenanceNatureCode,
        reason: reason.trim(),
      });
      setTarget(null);
      await load();
      void refreshWorkOrderTypesCatalog();
    } catch (err) {
      setSaveError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  if (loadError != null) {
    return (
      <div className="border-b border-surface-border p-3">
        <ErrorState error={loadError} onRetry={() => void load()} />
      </div>
    );
  }
  if (rows == null || rows.length === 0) return null;

  return (
    <section
      aria-labelledby="wo-type-nature-review-title"
      className="space-y-2 border-b border-tone-warning-border bg-tone-warning-bg p-3"
    >
      <h2 id="wo-type-nature-review-title" className="text-sm font-medium text-text-primary">
        {t("natureReview.title", { count: rows.length })}
      </h2>
      <p className="text-xs text-text-secondary">{t("natureReview.hint")}</p>
      <ul className="space-y-1 text-sm">
        {rows.map((row) => (
          <li key={row.id} className="flex flex-wrap items-center gap-2">
            <span className="font-medium text-text-primary">{row.label}</span>
            <span className="text-xs text-text-muted">{row.code}</span>
            <span className="text-xs text-text-muted">
              {t("natureReview.workOrders", { count: row.work_order_count })}
            </span>
            {!row.is_active ? (
              <span className="text-xs text-text-muted">{t("natureReview.inactive")}</span>
            ) : null}
            {canManage ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="ms-auto"
                onClick={() => open(row)}
              >
                {t("natureReview.give")}
              </Button>
            ) : null}
          </li>
        ))}
      </ul>

      <Dialog open={target != null} onOpenChange={(o) => (!o ? setTarget(null) : undefined)}>
        <DialogContent>
          <form onSubmit={(e) => void submit(e)} className="space-y-3">
            <DialogHeader>
              <DialogTitle>
                {t("natureReview.dialogTitle", { label: target?.label ?? "" })}
              </DialogTitle>
              <DialogDescription>{t("natureReview.dialogHint")}</DialogDescription>
            </DialogHeader>
            <FormField
              name="wo-type-nature"
              label={t("natureReview.nature")}
              required
              error={submitted && !nature ? t("natureReview.natureRequired") : undefined}
            >
              <Select value={nature} onValueChange={setNature}>
                <SelectTrigger id="wo-type-nature" aria-label={t("natureReview.nature")}>
                  <SelectValue placeholder={t("natureReview.naturePlaceholder")} />
                </SelectTrigger>
                <SelectContent>
                  {MAINTENANCE_NATURES.map((code) => (
                    <SelectItem key={code} value={code}>
                      {t(`natureReview.natures.${code}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField
              name="wo-type-nature-reason"
              label={t("natureReview.reason")}
              required
              error={
                submitted && reasonTooShort
                  ? t("natureReview.reasonMin", { min: NATURE_REASON_MIN })
                  : undefined
              }
            >
              <Textarea
                id="wo-type-nature-reason"
                rows={2}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </FormField>
            {saveError ? (
              <p role="alert" className="text-xs text-tone-danger-fg">
                {saveError}
              </p>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => setTarget(null)}>
                {t("natureReview.cancel")}
              </Button>
              <Button type="submit" disabled={busy}>
                {t("natureReview.save")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </section>
  );
}
