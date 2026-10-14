/** Small PM display pieces shared by the due queue, plan list, plan detail and calendar. */
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { PM_OCCURRENCE_STATUS_TONE, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import type { PmOccurrenceRow } from "@/services/pm-service";

import { isOccurrenceOverdue } from "./pm-model";

/** Asset code over its name; "—" when the plan has no single equipment. */
export function PmAssetCell({
  code,
  name,
}: {
  code: string | null | undefined;
  name: string | null | undefined;
}) {
  const { t } = useTranslation("pm");
  if (!code && !name) {
    return <span className="text-xs text-text-muted">{t("labels.none")}</span>;
  }
  return (
    <div className="min-w-0">
      {code ? <div className="font-mono text-xs">{code}</div> : null}
      {name ? <div className="max-w-[200px] truncate text-xs text-text-muted">{name}</div> : null}
    </div>
  );
}

/** "Asset code — name" for one-line places (dialog headers, overview). */
export function formatPmAsset(
  code: string | null | undefined,
  name: string | null | undefined,
): string | null {
  if (code && name) return `${code} — ${name}`;
  return code || name || null;
}

/** Due date, or the meter reading the occurrence is due at. */
export function PmDueLabel({ occurrence }: { occurrence: PmOccurrenceRow }) {
  const { t } = useTranslation("pm");
  const { formatDate, formatNumber } = useFormatters();
  if (occurrence.due_at) return <span className="text-xs">{formatDate(occurrence.due_at)}</span>;
  if (occurrence.due_meter_value != null) {
    return (
      <span className="text-xs">
        {t("labels.dueAtReading", { value: formatNumber(occurrence.due_meter_value) })}
      </span>
    );
  }
  return <span className="text-xs text-text-muted">{t("labels.none")}</span>;
}

/** Occurrence status chip, plus an "Overdue" chip for an open occurrence past its due date. */
export function PmOccurrenceStatus({ occurrence }: { occurrence: PmOccurrenceRow }) {
  const { t } = useTranslation("pm");
  return (
    <div className="flex flex-wrap items-center gap-1">
      <StatusChip
        label={t(`occurrenceStatus.${occurrence.status}`, { defaultValue: occurrence.status })}
        tone={toneOf(PM_OCCURRENCE_STATUS_TONE, occurrence.status)}
        dot
      />
      {isOccurrenceOverdue(occurrence) ? (
        <StatusChip label={t("labels.overdue")} tone="danger" />
      ) : null}
    </div>
  );
}
