/**
 * Supervisor review of a completed round (PRD §6.25 "Reviewed state"): every open anomaly gets
 * a decision (request, work order or deferral) before the round can be marked reviewed.
 */
import { ClipboardList, FileWarning, Hammer, Loader2, PauseCircle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Button,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import {
  deferInspectionAnomaly,
  reviewInspectionRound,
  routeInspectionAnomalyToDi,
  routeInspectionAnomalyToWo,
} from "@/services/inspection-service";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type { InspectionAnomaly, InspectionRoundRunner } from "@shared/ipc-types";

interface InspectionRoundReviewProps {
  runner: InspectionRoundRunner;
  onChanged: () => Promise<void>;
}

interface OpenFinding {
  anomaly: InspectionAnomaly;
  label: string;
}

export function InspectionRoundReview({ runner, onChanged }: InspectionRoundReviewProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const { formatDateTime } = useFormatters();
  const woTypes = useWorkOrderTypesCatalog((s) => s.types);
  const loadWoTypes = useWorkOrderTypesCatalog((s) => s.load);
  const activeWoTypes = useMemo(() => woTypes.filter((w) => w.is_active), [woTypes]);
  const [woType, setWoType] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadWoTypes();
  }, [loadWoTypes]);
  useEffect(() => {
    if (!woType && activeWoTypes.length > 0) setWoType(activeWoTypes[0]?.code ?? "");
  }, [activeWoTypes, woType]);

  const findings: OpenFinding[] = runner.checkpoints.flatMap((c) =>
    c.open_anomalies.map((anomaly) => ({
      anomaly,
      label: c.checkpoint.description ?? c.checkpoint.checkpoint_code,
    })),
  );
  const round = runner.summary.round;
  const canDecide = can("ins.execute");
  const canReview = can("ins.admin");

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      await onChanged();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  if (round.status === "reviewed") {
    return (
      <p className="text-sm text-text-muted">
        {t("review.reviewedBy", {
          name: runner.summary.reviewed_by_name ?? "—",
          date: formatDateTime(round.reviewed_at),
        })}
      </p>
    );
  }

  return (
    <section
      className="space-y-4 rounded-lg border border-surface-border bg-surface-1 p-4"
      aria-label={t("review.title")}
    >
      <h3 className="flex items-center gap-2 text-base font-semibold">
        <ClipboardList className="h-5 w-5" aria-hidden />
        {t("review.title")}
      </h3>

      {findings.length === 0 ? (
        <p className="text-sm text-text-muted">{t("review.none")}</p>
      ) : (
        <ul className="space-y-3">
          {findings.map(({ anomaly, label }) => (
            <li
              key={anomaly.id}
              className="space-y-2 rounded-md border border-status-warning/40 p-3"
              data-testid="review-finding"
            >
              <p className="flex items-center gap-2 text-sm font-medium">
                <FileWarning className="h-4 w-4 text-status-warning" aria-hidden />
                {label} —{" "}
                {t(`review.anomalyType.${anomaly.anomaly_type}`, {
                  defaultValue: anomaly.description,
                })}{" "}
                <span className="text-text-muted">
                  ({t("review.severity", { level: anomaly.severity })})
                </span>
              </p>
              {canDecide ? (
                <div className="flex flex-wrap gap-2">
                  <Button
                    size="lg"
                    variant="outline"
                    className="min-h-12"
                    disabled={busy}
                    onClick={() =>
                      void act(() =>
                        routeInspectionAnomalyToDi({
                          anomaly_id: anomaly.id,
                          expected_row_version: anomaly.row_version,
                          title: null,
                          description: null,
                        }),
                      )
                    }
                  >
                    {t("review.toDi")}
                  </Button>
                  <Button
                    size="lg"
                    variant="outline"
                    className="min-h-12"
                    disabled={busy || !woType}
                    onClick={() =>
                      void act(() =>
                        routeInspectionAnomalyToWo({
                          anomaly_id: anomaly.id,
                          expected_row_version: anomaly.row_version,
                          type_code: woType,
                          title: null,
                        }),
                      )
                    }
                  >
                    <Hammer className="mr-2 h-4 w-4" aria-hidden />
                    {t("review.toWo")}
                  </Button>
                  <Button
                    size="lg"
                    variant="ghost"
                    className="min-h-12"
                    disabled={busy}
                    onClick={() =>
                      void act(() =>
                        deferInspectionAnomaly({
                          anomaly_id: anomaly.id,
                          expected_row_version: anomaly.row_version,
                        }),
                      )
                    }
                  >
                    <PauseCircle className="mr-2 h-4 w-4" aria-hidden />
                    {t("review.defer")}
                  </Button>
                </div>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      {findings.length > 0 && canDecide ? (
        <div className="max-w-xs space-y-1">
          <Label htmlFor="review-wo-type">{t("review.woType")}</Label>
          <Select value={woType} onValueChange={setWoType}>
            <SelectTrigger id="review-wo-type">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {activeWoTypes.map((w) => (
                <SelectItem key={w.code} value={w.code}>
                  {w.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      ) : null}

      {canReview ? (
        <div className="space-y-2 border-t border-surface-border pt-4">
          <Label htmlFor="review-note">{t("review.note")}</Label>
          <Textarea
            id="review-note"
            rows={2}
            placeholder={t("review.notePlaceholder")}
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
          <Button
            size="lg"
            className="min-h-12"
            disabled={busy || findings.length > 0}
            onClick={() =>
              void act(() =>
                reviewInspectionRound({
                  round_id: round.id,
                  expected_row_version: round.row_version,
                  note: note.trim() || null,
                }),
              )
            }
          >
            {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
            {t("review.submit")}
          </Button>
        </div>
      ) : null}

      {error ? (
        <p role="alert" className="text-sm text-status-danger">
          {error}
        </p>
      ) : null}
    </section>
  );
}
