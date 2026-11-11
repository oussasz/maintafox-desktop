/**
 * The facts of a dépannage on the work order sheet (L1-05): "Panne constatée à", "Machine arrêtée",
 * its after-the-fact validation and its times.
 * - The breakdown time can be corrected until the work order is closed or cancelled
 *   (`set_wo_breakdown_at`, ot.edit at the work order's site; audited).
 * - "Dépannage — à valider" until an approver of the site (ot.approve) who did not start it
 *   validates it (`validate_depannage_wo`); a site that opted out approves it automatically at the
 *   close (shown as such, never as a person's approval).
 * - Response time, active repair time and time to restore (EN 13306, `get_depannage_times`).
 */

import { CheckCircle2, Loader2, Pencil } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { fromDateTimeLocal, toDateTimeLocal } from "@/components/planning/planning-model";
import { Button } from "@/components/ui/button";
import { DateTimeField } from "@/components/ui/date-time-field";
import { describeAppError } from "@/lib/app-error";
import { getDepannageTimes, setWoBreakdownAt, validateDepannageWo } from "@/services/wo-service";
import { formatDate } from "@/utils/format-date";
import type { DepannageTimes, WorkOrder } from "@shared/ipc-types";

/** Statuses after which the backend no longer changes the breakdown time nor validates. */
const TERMINAL = new Set(["closed", "cancelled"]);

interface WoDepannageFactsProps {
  wo: WorkOrder;
  /** ot.edit at the work order's site. */
  canEdit: boolean;
  /** ot.approve at the work order's site. */
  canApprove: boolean;
  /** The signed-in user (the starter never validates their own dépannage). */
  userId: number | null;
  /** The work order after a change (refresh the sheet). */
  onChanged: (wo: WorkOrder) => void;
}

export function WoDepannageFacts({
  wo,
  canEdit,
  canApprove,
  userId,
  onChanged,
}: WoDepannageFactsProps) {
  const { t, i18n } = useTranslation("ot");
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [validating, setValidating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [oldWarning, setOldWarning] = useState(false);
  const [times, setTimes] = useState<DepannageTimes | null>(null);

  useEffect(() => {
    if (!wo.is_depannage) return;
    let cancelled = false;
    getDepannageTimes(wo.id)
      .then((res) => {
        if (!cancelled) setTimes(res);
      })
      .catch(() => {
        if (!cancelled) setTimes(null);
      });
    return () => {
      cancelled = true;
    };
  }, [wo.id, wo.row_version, wo.is_depannage]);

  if (!wo.is_depannage) return null;
  const terminal = TERMINAL.has(wo.status_code ?? "");
  const editable = canEdit && !terminal;
  const validated = wo.validated_by_id != null;
  const autoApproved = wo.validation_auto_approved === true;
  const isStarter = userId != null && wo.requester_id === userId;

  const save = async () => {
    if (!value) return;
    setSaving(true);
    setError(null);
    try {
      const res = await setWoBreakdownAt({
        wo_id: wo.id,
        expected_row_version: wo.row_version,
        breakdown_at: fromDateTimeLocal(value),
      });
      setOldWarning(res.breakdown_old_warning);
      setEditing(false);
      onChanged(res.wo);
    } catch (err) {
      setError(describeAppError(err).title);
    } finally {
      setSaving(false);
    }
  };

  const validate = async () => {
    setValidating(true);
    setError(null);
    try {
      onChanged(await validateDepannageWo({ wo_id: wo.id, expected_row_version: wo.row_version }));
    } catch (err) {
      setError(describeAppError(err).title);
    } finally {
      setValidating(false);
    }
  };

  const hours = (h: number | null | undefined) =>
    h == null ? "—" : t("depannage.times.hours", { value: h.toFixed(1) });

  return (
    <div
      className="col-span-2 sm:col-span-3 space-y-1 rounded-md border border-tone-danger-border px-2 py-1.5"
      data-testid="wo-depannage-facts"
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="font-semibold text-tone-danger-fg">
          {validated || autoApproved
            ? t("depannage.badge")
            : t("depannage.validation.badgeToValidate")}
        </span>
        {editing ? (
          <span className="flex items-center gap-2">
            <span className="text-muted-foreground">{t("depannage.breakdownAt.label")}:</span>
            <DateTimeField
              id="wo-breakdown-at-edit"
              size="sm"
              value={value}
              onChange={setValue}
              aria-label={t("depannage.breakdownAt.label")}
            />
            <Button size="sm" onClick={() => void save()} disabled={saving || !value}>
              {saving ? <Loader2 className="h-3 w-3 animate-spin" aria-hidden /> : null}
              {t("depannage.breakdownAt.save")}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)} disabled={saving}>
              {t("form.cancel")}
            </Button>
          </span>
        ) : (
          <span className="flex items-center gap-1.5">
            <span className="text-muted-foreground">{t("depannage.breakdownAt.label")}:</span>
            <span className="font-medium">
              {wo.breakdown_at ? formatDate(wo.breakdown_at, i18n.language) : "—"}
            </span>
            {editable ? (
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-1.5"
                aria-label={t("depannage.breakdownAt.edit")}
                title={t("depannage.breakdownAt.edit")}
                onClick={() => {
                  setValue(wo.breakdown_at ? toDateTimeLocal(wo.breakdown_at) : "");
                  setError(null);
                  setEditing(true);
                }}
              >
                <Pencil className="h-3 w-3" aria-hidden />
              </Button>
            ) : null}
          </span>
        )}
        <span className="flex items-center gap-1.5">
          <span className="text-muted-foreground">{t("depannage.machineStopped.label")}:</span>
          <span className="font-medium">
            {wo.machine_stopped == null
              ? "—"
              : wo.machine_stopped
                ? t("depannage.machineStopped.yes")
                : t("depannage.machineStopped.no")}
          </span>
        </span>
      </div>

      <div
        className="flex flex-wrap items-center gap-x-3 gap-y-1"
        data-testid="wo-depannage-validation"
      >
        {validated ? (
          <span className="flex items-center gap-1 text-status-success">
            <CheckCircle2 className="h-3.5 w-3.5" aria-hidden />
            {t("depannage.validation.validated", {
              date: wo.validated_at ? formatDate(wo.validated_at, i18n.language) : "—",
            })}
          </span>
        ) : autoApproved ? (
          <span className="text-muted-foreground">{t("depannage.validation.autoApproved")}</span>
        ) : terminal ? null : (
          <>
            <span className="text-muted-foreground">{t("depannage.validation.pending")}</span>
            {canApprove && !isStarter ? (
              <Button
                size="sm"
                onClick={() => void validate()}
                disabled={validating}
                data-testid="wo-depannage-validate"
              >
                {validating ? <Loader2 className="h-3 w-3 animate-spin" aria-hidden /> : null}
                {t("depannage.validation.validate")}
              </Button>
            ) : canApprove && isStarter ? (
              <span className="text-xs text-muted-foreground">
                {t("depannage.validation.notStarter")}
              </span>
            ) : null}
          </>
        )}
      </div>

      {times ? (
        <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs" data-testid="wo-depannage-times">
          <span>
            <span className="text-muted-foreground">{t("depannage.times.response")}:</span>{" "}
            <span className="font-medium">{hours(times.response_hours)}</span>
          </span>
          <span>
            <span className="text-muted-foreground">{t("depannage.times.activeRepair")}:</span>{" "}
            <span className="font-medium">{hours(times.active_repair_hours)}</span>
          </span>
          <span>
            <span className="text-muted-foreground">{t("depannage.times.restore")}:</span>{" "}
            <span className="font-medium">{hours(times.restore_hours)}</span>
          </span>
        </div>
      ) : null}

      {oldWarning ? (
        <p className="text-status-warning" role="note">
          {t("depannage.result.oldBreakdown")}
        </p>
      ) : null}
      {error ? (
        <p className="text-tone-danger-fg" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}
