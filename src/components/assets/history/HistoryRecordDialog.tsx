import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
import type { ReferenceTypeId } from "@/components/reference/reference-types";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { saveWoHistoryCoding } from "@/services/asset-history-service";
import type { AssetHistoryEvent, WoHistoryCodingInput } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

type CodingKey = Exclude<keyof WoHistoryCodingInput, "record_id" | "expected_row_version">;

/** The coding fields, their reference list and their label key (same lists as an OT close-out). */
const CODING: { key: CodingKey; referenceType: ReferenceTypeId; labelKey: string }[] = [
  { key: "failure_mode_id", referenceType: "work.failure_mode", labelKey: "failureMode" },
  { key: "failure_cause_id", referenceType: "work.failure_cause", labelKey: "failureCause" },
  { key: "failure_mechanism_id", referenceType: "work.failure_mechanism", labelKey: "mechanism" },
  { key: "detection_method_id", referenceType: "work.detection_method", labelKey: "detection" },
  { key: "failure_effect_id", referenceType: "work.failure_effect", labelKey: "failureEffect" },
];

function idOf(event: AssetHistoryEvent, key: string): number | null {
  const v = event.metadata[key];
  return typeof v === "number" ? v : null;
}

function textOf(event: AssetHistoryEvent, key: string): string | null {
  const v = event.metadata[key];
  return typeof v === "string" && v.trim() ? v : null;
}

/**
 * An imported maintenance history record (L7-30): read-only, except its failure coding, which a
 * user with the work-order edit right at the equipment's site may change (checked again by the
 * backend at the equipment's site).
 */
export function HistoryRecordDialog({
  event,
  onClose,
  onSaved,
}: {
  event: AssetHistoryEvent | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("equipment");
  const { can } = usePermissions();
  const { formatDateTime, formatCurrency } = useFormatters();
  const [coding, setCoding] = useState<Record<CodingKey, number | null>>({
    failure_mode_id: null,
    failure_cause_id: null,
    failure_mechanism_id: null,
    detection_method_id: null,
    failure_effect_id: null,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!event) return;
    setCoding({
      failure_mode_id: idOf(event, "failure_mode_id"),
      failure_cause_id: idOf(event, "failure_cause_id"),
      failure_mechanism_id: idOf(event, "failure_mechanism_id"),
      detection_method_id: idOf(event, "detection_method_id"),
      failure_effect_id: idOf(event, "failure_effect_id"),
    });
    setError(null);
  }, [event]);

  if (!event) return null;
  const recordId = event.ref?.entity_id ?? null;
  const rowVersion = idOf(event, "row_version");
  const canEdit = can(P.OT_EDIT) && recordId != null && rowVersion != null;
  const startedAt = textOf(event, "started_at");
  const totalCost = idOf(event, "total_cost");

  const save = async () => {
    if (recordId == null || rowVersion == null) return;
    setSaving(true);
    setError(null);
    try {
      await saveWoHistoryCoding({
        record_id: recordId,
        expected_row_version: rowVersion,
        ...coding,
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(describeAppError(err).title);
    } finally {
      setSaving(false);
    }
  };

  const facts: [string, string | null][] = [
    [t("history.record.nature"), textOf(event, "nature_label")],
    [t("history.record.started"), startedAt ? formatDateTime(startedAt) : null],
    [t("history.record.ended"), formatDateTime(event.occurred_at)],
    [
      t("history.record.failure"),
      event.metadata["is_failure"] === true ? t("history.record.yes") : t("history.record.no"),
    ],
    [
      t("history.record.downtime"),
      event.duration_minutes != null ? `${(event.duration_minutes / 60).toFixed(1)} h` : null,
    ],
    [t("history.record.cost"), totalCost != null ? formatCurrency(totalCost) : null],
    [t("history.record.description"), event.description],
    [t("history.record.workDone"), textOf(event, "work_done")],
    [t("history.record.interveners"), textOf(event, "interveners_text")],
  ];

  return (
    <Dialog open onOpenChange={(open) => (!open ? onClose() : undefined)}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {t("history.titles.history_imported", { code: event.ref?.entity_code ?? "" })}
          </DialogTitle>
          <DialogDescription>{t("history.record.readOnly")}</DialogDescription>
        </DialogHeader>

        <dl className="grid grid-cols-1 gap-x-4 gap-y-2 text-sm sm:grid-cols-[auto_1fr]">
          {facts
            .filter(([, value]) => value != null)
            .map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-muted-foreground">{label}</dt>
                <dd className="whitespace-pre-wrap">{value}</dd>
              </div>
            ))}
        </dl>

        <section className="space-y-3">
          <h3 className="text-sm font-medium">{t("history.record.coding")}</h3>
          <div className="grid gap-3 sm:grid-cols-2">
            {CODING.map(({ key, referenceType, labelKey }) => (
              <div key={key} className="space-y-1">
                <Label htmlFor={`history-${key}`}>{t(`history.record.${labelKey}`)}</Label>
                <ReferenceCombobox
                  id={`history-${key}`}
                  referenceType={referenceType}
                  valueMode="id"
                  value={coding[key] != null ? String(coding[key]) : null}
                  onChange={(idStr) =>
                    setCoding((prev) => ({ ...prev, [key]: idStr ? Number(idStr) : null }))
                  }
                  allowClear
                  allowCreate={false}
                  disabled={!canEdit || saving}
                />
              </div>
            ))}
          </div>
          {!canEdit ? (
            <p className="text-xs text-muted-foreground">{t("history.record.noRight")}</p>
          ) : null}
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
        </section>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            {t("history.record.close")}
          </Button>
          {canEdit ? (
            <Button onClick={() => void save()} disabled={saving}>
              {saving ? t("history.record.saving") : t("history.record.save")}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
