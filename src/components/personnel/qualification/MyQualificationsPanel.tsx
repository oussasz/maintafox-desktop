/**
 * The signed-in person's own certifications and training sessions (self-service; needs a
 * login linked to a personnel record).
 */
import { UserRound } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import {
  listMyPersonnelCertifications,
  listMyTrainingSessions,
} from "@/services/qualification-service";
import type { PersonnelCertification, TrainingAttendance } from "@shared/ipc-types";

import { ATTENDANCE_TONE } from "./TrainingSessionsPanel";
import { CERT_READINESS_TONE } from "./qualification-tones";

export function MyQualificationsPanel() {
  const { t } = useTranslation("training");
  const { formatDate } = useFormatters();
  const [certs, setCerts] = useState<PersonnelCertification[]>([]);
  const [sessions, setSessions] = useState<TrainingAttendance[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [c, s] = await Promise.all([listMyPersonnelCertifications(), listMyTrainingSessions()]);
      setCerts(c);
      setSessions(s);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (loading) return <LoadingState />;
  if (error) return <ErrorState error={error} onRetry={() => void load()} />;
  if (certs.length === 0 && sessions.length === 0) {
    return (
      <EmptyState
        icon={<UserRound className="h-8 w-8" aria-hidden />}
        title={t("mine.empty")}
        description={t("mine.emptyHint")}
      />
    );
  }

  return (
    <div className="grid gap-6 lg:grid-cols-2">
      <section className="space-y-2">
        <h3 className="text-sm font-semibold text-text-primary">{t("mine.certifications")}</h3>
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {certs.map((c) => (
            <li
              key={c.id}
              className="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-sm"
            >
              <span>
                <span className="font-medium text-text-primary">
                  {c.certification_type_name ?? c.certification_type_code}
                </span>
                <span className="ml-2 text-xs text-text-muted">
                  {c.expires_at
                    ? t("mine.expiresOn", { date: formatDate(c.expires_at) })
                    : t("sheet.noExpiry")}
                </span>
              </span>
              <StatusChip
                label={t(`certStatus.${c.readiness_status}`, {
                  defaultValue: t("certStatus.pending"),
                })}
                tone={toneOf(CERT_READINESS_TONE, c.readiness_status)}
                dot
              />
            </li>
          ))}
        </ul>
      </section>
      <section className="space-y-2">
        <h3 className="text-sm font-semibold text-text-primary">{t("mine.sessions")}</h3>
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {sessions.map((s) => (
            <li
              key={s.id}
              className="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-sm"
            >
              <span className="text-text-primary">
                {s.completed_at
                  ? t("mine.completedOn", { date: formatDate(s.completed_at) })
                  : t("mine.notCompleted")}
                {s.score != null ? ` · ${t("mine.score", { score: s.score })}` : ""}
              </span>
              <StatusChip
                label={t(`attendance.${s.attendance_status}`, {
                  defaultValue: s.attendance_status,
                })}
                tone={ATTENDANCE_TONE[s.attendance_status] ?? "neutral"}
                dot
              />
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
