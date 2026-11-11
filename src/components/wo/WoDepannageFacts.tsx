/**
 * The facts of a dépannage on the work order sheet (L1-05): "Panne constatée à" and
 * "Machine arrêtée". The breakdown time can be corrected until the work order is closed or
 * cancelled (`set_wo_breakdown_at`, ot.edit at the work order's site; audited).
 */

import { Loader2, Pencil } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { fromDateTimeLocal, toDateTimeLocal } from "@/components/planning/planning-model";
import { Button } from "@/components/ui/button";
import { DateTimeField } from "@/components/ui/date-time-field";
import { describeAppError } from "@/lib/app-error";
import { setWoBreakdownAt } from "@/services/wo-service";
import { formatDate } from "@/utils/format-date";
import type { WorkOrder } from "@shared/ipc-types";

/** Statuses after which the backend no longer changes the breakdown time. */
const TERMINAL = new Set(["closed", "cancelled"]);

interface WoDepannageFactsProps {
  wo: WorkOrder;
  /** ot.edit at the work order's site. */
  canEdit: boolean;
  /** The work order after a change (refresh the sheet). */
  onChanged: (wo: WorkOrder) => void;
}

export function WoDepannageFacts({ wo, canEdit, onChanged }: WoDepannageFactsProps) {
  const { t, i18n } = useTranslation("ot");
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [oldWarning, setOldWarning] = useState(false);

  if (!wo.is_depannage) return null;
  const editable = canEdit && !TERMINAL.has(wo.status_code ?? "");

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

  return (
    <div
      className="col-span-2 sm:col-span-3 space-y-1 rounded-md border border-tone-danger-border px-2 py-1.5"
      data-testid="wo-depannage-facts"
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <span className="font-semibold text-tone-danger-fg">{t("depannage.badge")}</span>
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
