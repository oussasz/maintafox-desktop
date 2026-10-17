/**
 * Study step 3 — FMECA of the study equipment: worksheet of ISO 14224 failure modes with
 * severity / occurrence / detection on the plant grids (occurrence proposed from the life
 * analysis or the history, with its basis), criticality matrix, and the root-cause diagrams
 * (Ishikawa) of the equipment.
 */
import { ArrowRight, Pencil, Plus, SlidersHorizontal, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { IshikawaDiagramCanvas } from "@/components/reliability/IshikawaDiagramCanvas";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import {
  RELIABILITY_DECISION_STATUS_TONE,
  RELIABILITY_TACTIC_TONE,
  RELIABILITY_ZONE_TONE,
  mfTone,
  toneOf,
} from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import {
  type FmecaRow,
  type FmecaWorksheet,
  type ModeProposal,
  deleteReliabilityStudyFmecaItem,
  getReliabilityStudyFmeca,
} from "@/services/reliability-model-service";
import type { ReliabilityStudy } from "@/services/reliability-study-service";
import { P } from "@shared/rbac/permissions.generated";

import { type FmecaDraft, FmecaItemDialog } from "./FmecaItemDialog";
import { RatingScalesDialog } from "./RatingScalesDialog";
import {
  RAMS_SELECT_CLASS,
  RAMS_TABLE_FRAME,
  RamsCallout,
  RamsCard,
  useScaleLabel,
} from "./rams-ui";

export function StudyFmecaStep({
  study,
  editable,
  onGoRcm,
}: {
  study: ReliabilityStudy;
  editable: boolean;
  onGoRcm: () => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal } = useFormatters();
  const scaleLabel = useScaleLabel();
  const { can } = usePermissions();
  const [ws, setWs] = useState<FmecaWorksheet | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [draft, setDraft] = useState<FmecaDraft | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<FmecaRow | null>(null);
  const [scalesOpen, setScalesOpen] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setWs(await getReliabilityStudyFmeca(study.id));
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [study.id]);

  useEffect(() => {
    void load();
  }, [load]);

  if (error != null && ws == null)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (ws == null) return <LoadingState />;

  const canEdit = editable && ws.editable;
  const level = (kind: "severity" | "occurrence" | "detection", score: number) =>
    ws.scales[kind].find((l) => l.score === score);

  const fromProposal = (p: ModeProposal): FmecaDraft => ({
    itemId: null,
    rowVersion: null,
    failureModeId: p.failure_mode_id,
    componentLabel: "",
    functionalFailure: "",
    effect: "",
    mechanismId: p.mechanism?.id ?? null,
    causeId: p.cause?.id ?? null,
    severity: null,
    occurrence: p.occurrence.score,
    detectability: null,
    currentControl: "",
    recommendedAction: "",
    occurrenceSource: p.occurrence.source as FmecaDraft["occurrenceSource"],
    occurrenceEventsPerYear: p.occurrence.events_per_year,
    proposal: p.occurrence,
    rootCause: null,
  });

  const fromRow = (r: FmecaRow): FmecaDraft => ({
    itemId: r.item_id,
    rowVersion: r.row_version,
    failureModeId: r.failure_mode_id,
    componentLabel: r.component_label ?? "",
    functionalFailure: r.functional_failure,
    effect: r.failure_effect,
    mechanismId: r.failure_mechanism_id,
    causeId: r.failure_cause_id,
    severity: r.severity,
    occurrence: r.occurrence,
    detectability: r.detectability,
    currentControl: r.current_control,
    recommendedAction: r.recommended_action,
    occurrenceSource: (r.occurrence_source ?? "analyst") as FmecaDraft["occurrenceSource"],
    occurrenceEventsPerYear: r.occurrence_events_per_year,
    proposal: r.occurrence_proposal,
    rootCause: null,
  });

  /** Empty row: nothing is rated until the analyst chooses (no default rating). */
  const blankDraft = (rootCause: FmecaDraft["rootCause"]): FmecaDraft => ({
    itemId: null,
    rowVersion: null,
    failureModeId: null,
    componentLabel: "",
    functionalFailure: "",
    effect: "",
    mechanismId: null,
    causeId: null,
    severity: null,
    occurrence: null,
    detectability: null,
    currentControl: "",
    recommendedAction: "",
    occurrenceSource: "analyst",
    occurrenceEventsPerYear: null,
    proposal: null,
    rootCause,
  });
  const canAnalyze = canEdit && can(P.RAM_ANALYZE);

  const basisText = (p: {
    source: string;
    events_per_year: number;
    failures: number;
    equipment_years: number;
    mean_life_hours: number | null;
    hours_per_equipment_year: number | null;
  }) =>
    p.source === "weibull"
      ? t("rams.fmeca.basisWeibull", {
          rate: formatDecimal(p.events_per_year, 2),
          mttf: formatDecimal(p.mean_life_hours ?? 0, 0),
          hours: formatDecimal(p.hours_per_equipment_year ?? 0, 0),
        })
      : t("rams.fmeca.basisHistory", {
          rate: formatDecimal(p.events_per_year, 2),
          failures: p.failures,
          years: formatDecimal(p.equipment_years, 1),
        });

  const classes = ws.scales.matrix.classes;
  return (
    <div className="grid gap-4">
      <div className="grid gap-4 xl:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
        <RamsCard
          title={t("rams.fmeca.title", {
            classes: ws.class_names.join(", ") || study.assets.map((a) => a.asset_code).join(", "),
          })}
          subtitle={t("rams.fmeca.hint")}
          headingId="rams-fmeca"
          actions={
            <Button variant="outline" className="h-11" onClick={() => setScalesOpen(true)}>
              <SlidersHorizontal className="h-4 w-4" aria-hidden />
              {t("rams.fmeca.scales")}
            </Button>
          }
        >
          {ws.rows.length === 0 ? (
            <EmptyState
              title={t("rams.fmeca.emptyTitle")}
              description={t("rams.fmeca.emptyBody")}
            />
          ) : (
            <div className={RAMS_TABLE_FRAME}>
              <div className="overflow-x-auto">
                <table className="w-full text-sm" aria-labelledby="rams-fmeca">
                  <thead>
                    <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                      <th className="px-3 py-2 text-start">{t("rams.fmeca.colElement")}</th>
                      <th className="px-3 py-2 text-start">{t("rams.fmeca.colMode")}</th>
                      <th className="px-3 py-2 text-start">{t("rams.fmeca.colEffect")}</th>
                      <th className="px-3 py-2 text-start">{t("rams.fmeca.colMechanism")}</th>
                      <th className="px-3 py-2 text-end" title={t("rams.fmeca.severity")}>
                        {t("rams.fmeca.colS")}
                      </th>
                      <th className="px-3 py-2 text-end" title={t("rams.fmeca.occurrence")}>
                        {t("rams.fmeca.colO")}
                      </th>
                      <th className="px-3 py-2 text-end" title={t("rams.fmeca.detection")}>
                        {t("rams.fmeca.colD")}
                      </th>
                      <th className="px-3 py-2 text-end">{t("rams.fmeca.colRpn")}</th>
                      <th className="px-3 py-2 text-start">{t("rams.fmeca.colAction")}</th>
                      <th className="px-3 py-2">
                        <span className="sr-only">{t("rams.fmeca.colEdit")}</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {ws.rows.map((r) => (
                      <tr key={r.item_id} className="h-11 border-t border-surface-border align-top">
                        <td className="px-3 py-2">
                          {r.component_label || "—"}
                          {r.functional_failure ? (
                            <div className="text-xs text-text-muted">
                              {t("rams.fmeca.functionalFailureShort", {
                                text: r.functional_failure,
                              })}
                            </div>
                          ) : null}
                        </td>
                        <td className="px-3 py-2">
                          <span className="font-mono text-xs">{r.mode_code}</span> {r.mode_label}
                        </td>
                        <td className="px-3 py-2 text-text-secondary">{r.failure_effect || "—"}</td>
                        <td className="px-3 py-2 text-text-secondary">
                          {[r.mechanism_label, r.cause_label].filter(Boolean).join(" · ") || "—"}
                        </td>
                        <td
                          className="px-3 py-2 text-end font-mono"
                          title={scaleLabel(level("severity", r.severity))}
                        >
                          {r.severity}
                        </td>
                        <td
                          className="px-3 py-2 text-end font-mono"
                          title={`${scaleLabel(level("occurrence", r.occurrence))}${
                            r.occurrence_source &&
                            r.occurrence_source !== "analyst" &&
                            r.occurrence_proposal
                              ? ` — ${basisText(r.occurrence_proposal)}`
                              : ""
                          }`}
                        >
                          {r.occurrence}
                          {r.occurrence_source === "weibull" ? (
                            <span className="ms-1 text-[10px] text-text-muted">
                              {t("rams.fmeca.weibullTag")}
                            </span>
                          ) : null}
                        </td>
                        <td
                          className="px-3 py-2 text-end font-mono"
                          title={scaleLabel(level("detection", r.detectability))}
                        >
                          {r.detectability}
                        </td>
                        <td className="px-3 py-2 text-end">
                          {r.review_required ? (
                            <StatusChip
                              tone="attention"
                              label={t("rams.fmeca.toReview")}
                              title={t("rams.fmeca.toReviewHint")}
                            />
                          ) : (
                            <StatusChip
                              tone={toneOf(RELIABILITY_ZONE_TONE, r.zone)}
                              label={String(r.rpn)}
                              title={t(`rams.fmeca.zone.${r.zone}`)}
                            />
                          )}
                        </td>
                        <td className="px-3 py-2">
                          {r.decision_status ? (
                            <StatusChip
                              tone={toneOf(RELIABILITY_DECISION_STATUS_TONE, r.decision_status)}
                              label={t(`rams.decisions.status.${r.decision_status}`)}
                            />
                          ) : r.rcm_tactic ? (
                            <StatusChip
                              tone={toneOf(RELIABILITY_TACTIC_TONE, r.rcm_tactic)}
                              label={t(`rams.rcm.tactic.${r.rcm_tactic}`)}
                            />
                          ) : (
                            <StatusChip tone="neutral" label={t("rams.fmeca.noAction")} />
                          )}
                        </td>
                        <td className="px-3 py-2">
                          {canEdit && can(P.RAM_ANALYZE) ? (
                            <div className="flex justify-end gap-1">
                              <Button
                                variant="ghost"
                                className="h-10 w-10 p-0"
                                aria-label={t("rams.fmeca.editRow", { mode: r.mode_label ?? "" })}
                                onClick={() => setDraft(fromRow(r))}
                              >
                                <Pencil className="h-4 w-4" aria-hidden />
                              </Button>
                              <Button
                                variant="ghost"
                                className="h-10 w-10 p-0"
                                aria-label={t("rams.fmeca.deleteRow", { mode: r.mode_label ?? "" })}
                                disabled={r.decision_id != null || r.rcm_decision_id != null}
                                title={
                                  r.decision_id != null || r.rcm_decision_id != null
                                    ? t("rams.fmeca.deleteBlocked")
                                    : undefined
                                }
                                onClick={() => setDeleteTarget(r)}
                              >
                                <Trash2 className="h-4 w-4" aria-hidden />
                              </Button>
                            </div>
                          ) : null}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}
          {ws.rows_to_review > 0 ? (
            <div className="mt-3">
              <RamsCallout
                tone="attention"
                title={t("rams.fmeca.rowsToReview", { count: ws.rows_to_review })}
              >
                {t("rams.fmeca.rowsToReviewBody")}
              </RamsCallout>
            </div>
          ) : null}
          <div className="mt-3 flex flex-wrap justify-between gap-2">
            {canEdit && can(P.RAM_ANALYZE) ? (
              <Button variant="outline" className="h-11" onClick={() => setDraft(blankDraft(null))}>
                <Plus className="h-4 w-4" aria-hidden />
                {t("rams.fmeca.addOther")}
              </Button>
            ) : (
              <span />
            )}
            <Button
              className={mfLayout.primaryAction}
              onClick={onGoRcm}
              disabled={ws.rows.length === 0}
            >
              {t("rams.fmeca.toRcm")}
              <ArrowRight className="h-4 w-4" aria-hidden />
            </Button>
          </div>
        </RamsCard>

        <div className="grid content-start gap-4">
          <RamsCard
            title={t("rams.fmeca.matrixTitle")}
            subtitle={t("rams.fmeca.matrixHint", { classes })}
          >
            <div
              className="grid gap-1 text-[11px]"
              style={{ gridTemplateColumns: `2rem repeat(${classes}, minmax(0, 1fr))` }}
              role="table"
              aria-label={t("rams.fmeca.matrixTitle")}
            >
              <div role="row" className="contents">
                <span role="columnheader" />
                {Array.from({ length: classes }, (_, i) => (
                  <span key={i} role="columnheader" className="text-center text-text-muted">
                    {t("rams.fmeca.occurrenceClass", { n: i + 1 })}
                  </span>
                ))}
              </div>
              {Array.from({ length: classes }, (_, si) => classes - si).map((sc) => (
                <div key={sc} role="row" className="contents">
                  <span role="rowheader" className="flex items-center text-text-muted">
                    {t("rams.fmeca.severityClass", { n: sc })}
                  </span>
                  {Array.from({ length: classes }, (_, oi) => oi + 1).map((oc) => {
                    const cell = ws.matrix.find(
                      (m) => m.severity_class === sc && m.occurrence_class === oc,
                    );
                    const tone = toneOf(RELIABILITY_ZONE_TONE, cell?.zone);
                    return (
                      <span
                        key={oc}
                        role="cell"
                        title={t("rams.fmeca.cellTitle", {
                          s: sc,
                          o: oc,
                          zone: t(`rams.fmeca.zone.${cell?.zone ?? "acceptable"}`),
                          count: cell?.count ?? 0,
                        })}
                        className={cn(
                          "flex aspect-[1.7] items-center justify-center rounded-md border font-mono text-sm font-semibold",
                          mfTone[tone].chip,
                        )}
                      >
                        {cell && cell.count > 0 ? cell.count : ""}
                      </span>
                    );
                  })}
                </div>
              ))}
            </div>
            <div className="mt-3 flex flex-wrap gap-2 text-xs">
              {["unacceptable", "reduce", "monitor", "acceptable"].map((z) => (
                <StatusChip
                  key={z}
                  tone={toneOf(RELIABILITY_ZONE_TONE, z)}
                  label={t(`rams.fmeca.zone.${z}`)}
                />
              ))}
            </div>
            <p className="mt-2 text-xs text-text-muted">
              {t("rams.fmeca.rpnThreshold", { threshold: ws.rpn_threshold })}
            </p>
            {ws.rows_to_review > 0 ? (
              <p className="mt-1 text-xs font-medium text-tone-attention-fg">
                {t("rams.fmeca.rowsToReview", { count: ws.rows_to_review })}
              </p>
            ) : null}
          </RamsCard>

          <RamsCard title={t("rams.fmeca.proposalsTitle")} subtitle={t("rams.fmeca.proposalsHint")}>
            {ws.proposals.length === 0 ? (
              <p className="text-sm text-text-muted">{t("rams.fmeca.noProposals")}</p>
            ) : (
              <ul className="grid gap-2">
                {ws.proposals.map((p) => (
                  <li
                    key={p.failure_mode_id}
                    className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-surface-border px-3 py-2"
                  >
                    <div className="min-w-0">
                      <p className="text-sm font-medium text-text-primary">
                        <span className="font-mono text-xs">{p.code}</span> {p.label}
                      </p>
                      <p className="text-xs text-text-muted">
                        {t("rams.fmeca.proposalCounts", {
                          period: p.failures_in_period,
                          classCount: p.failures_on_class,
                        })}{" "}
                        · {t("rams.fmeca.proposedO", { score: p.occurrence.score })} —{" "}
                        {basisText(p.occurrence)}
                      </p>
                    </div>
                    {canEdit && can(P.RAM_ANALYZE) ? (
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => setDraft(fromProposal(p))}
                      >
                        <Plus className="h-4 w-4" aria-hidden />
                        {t("rams.fmeca.add")}
                      </Button>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </RamsCard>
        </div>
      </div>

      <RootCausePanel
        study={study}
        {...(canAnalyze
          ? {
              onPromoteToFmeca: (link: NonNullable<FmecaDraft["rootCause"]>) =>
                setDraft(blankDraft(link)),
            }
          : {})}
      />

      {draft ? (
        <FmecaItemDialog
          open
          onOpenChange={(o) => !o && setDraft(null)}
          studyId={study.id}
          worksheet={ws}
          draft={draft}
          onSaved={(next) => {
            setWs(next);
            setDraft(null);
          }}
        />
      ) : null}
      <ConfirmDialog
        open={deleteTarget != null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
        description={t("rams.fmeca.deleteConfirm", { mode: deleteTarget?.mode_label ?? "" })}
        confirmLabel={t("rams.fmeca.deleteAction")}
        destructive
        onConfirm={async () => {
          if (!deleteTarget) return;
          setWs(await deleteReliabilityStudyFmecaItem(study.id, deleteTarget.item_id));
        }}
      />
      <RatingScalesDialog
        open={scalesOpen}
        onOpenChange={setScalesOpen}
        scales={ws.scales}
        editable={can(P.RAM_MANAGE)}
        onSaved={() => void load()}
      />
    </div>
  );
}

/** Ishikawa root-cause diagrams of the study equipment (former governance screen). */
function RootCausePanel({
  study,
  onPromoteToFmeca,
}: {
  study: ReliabilityStudy;
  /** Opens this study's FMECA form for a diagram cause; absent without the right to analyse. */
  onPromoteToFmeca?: (link: NonNullable<FmecaDraft["rootCause"]>) => void;
}) {
  const { t } = useTranslation("reliability");
  const [equipmentId, setEquipmentId] = useState<number | null>(
    study.assets[0]?.equipment_id ?? null,
  );
  const options = useMemo(() => study.assets, [study.assets]);
  /** The open diagram holds moves, links or deletions not saved yet. */
  const [diagramDirty, setDiagramDirty] = useState(false);
  /** Equipment picked while the open diagram has unsaved changes (asks before dropping them). */
  const [pendingEquipmentId, setPendingEquipmentId] = useState<number | null>(null);
  const pickEquipment = (id: number) => {
    if (id === equipmentId) return;
    if (diagramDirty) setPendingEquipmentId(id);
    else setEquipmentId(id);
  };
  return (
    <RamsCard
      title={t("rams.fmeca.rootCauseTitle")}
      subtitle={t("rams.fmeca.rootCauseHint")}
      actions={
        options.length > 1 ? (
          <label className="flex items-center gap-2 text-xs text-text-secondary">
            {t("rams.fmeca.rootCauseEquipment")}
            <select
              className={RAMS_SELECT_CLASS}
              value={equipmentId ?? ""}
              onChange={(e) => pickEquipment(Number(e.target.value))}
            >
              {options.map((a) => (
                <option key={a.equipment_id} value={a.equipment_id}>
                  {`${a.asset_code} · ${a.asset_name}`}
                </option>
              ))}
            </select>
          </label>
        ) : null
      }
    >
      {equipmentId != null ? (
        <div className="h-[900px] overflow-hidden rounded-md border border-surface-border">
          <IshikawaDiagramCanvas
            key={equipmentId}
            equipmentId={equipmentId}
            {...(onPromoteToFmeca ? { onPromoteToFmeca } : {})}
            onDirtyChange={setDiagramDirty}
          />
        </div>
      ) : null}
      <ConfirmDialog
        open={pendingEquipmentId != null}
        onOpenChange={(open) => {
          if (!open) setPendingEquipmentId(null);
        }}
        title={t("governance.unsavedTitle")}
        description={t("governance.unsavedBody")}
        confirmLabel={t("governance.unsavedDiscard")}
        destructive
        onConfirm={() => {
          if (pendingEquipmentId != null) setEquipmentId(pendingEquipmentId);
        }}
      />
    </RamsCard>
  );
}
