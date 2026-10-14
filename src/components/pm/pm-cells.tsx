/** Small PM display pieces shared by the due queue, plan list, plan detail and calendar. */
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import { PM_OCCURRENCE_STATUS_TONE, PM_TIMING_TONE, toneOf } from "@/design-system/tone";
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

/** Due date (and meter threshold), with the tolerance window when it is wider than the day. */
export function PmDueLabel({ occurrence }: { occurrence: PmOccurrenceRow }) {
  const { t } = useTranslation("pm");
  const { formatDate, formatNumber } = useFormatters();
  const lines: string[] = [];
  if (occurrence.window_start_meter != null && occurrence.window_end_meter != null) {
    lines.push(
      t("labels.meterWindow", {
        from: formatNumber(occurrence.window_start_meter),
        to: formatNumber(occurrence.window_end_meter),
      }),
    );
  }
  if (occurrence.window_start_at && occurrence.window_end_at) {
    lines.push(
      t("labels.window", {
        from: formatDate(occurrence.window_start_at),
        to: formatDate(occurrence.window_end_at),
      }),
    );
  }
  const main = occurrence.due_at
    ? formatDate(occurrence.due_at)
    : occurrence.due_meter_value != null
      ? t("labels.dueAtReading", { value: formatNumber(occurrence.due_meter_value) })
      : null;
  if (!main) return <span className="text-xs text-text-muted">{t("labels.none")}</span>;
  return (
    <div className="text-xs">
      <div>{main}</div>
      {occurrence.due_at && occurrence.due_meter_value != null ? (
        <div className="text-text-muted">
          {t("labels.dueAtReading", { value: formatNumber(occurrence.due_meter_value) })}
        </div>
      ) : null}
      {lines.map((line) => (
        <div key={line} className="text-text-muted">
          {line}
        </div>
      ))}
    </div>
  );
}

/** On time / early / late chip of completed work. */
export function PmTimingChip({ timing }: { timing: string | null | undefined }) {
  const { t } = useTranslation("pm");
  if (!timing) return null;
  return (
    <StatusChip
      label={t(`timing.${timing}`, { defaultValue: timing })}
      tone={toneOf(PM_TIMING_TONE, timing)}
    />
  );
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
      {occurrence.status === "completed" ? (
        <PmTimingChip timing={occurrence.timing_status} />
      ) : null}
    </div>
  );
}
