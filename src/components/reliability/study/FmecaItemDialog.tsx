/**
 * Add or edit one FMECA row of a study: element, ISO 14224 mode / mechanism / cause, effect,
 * severity / occurrence / detection on the plant grids (each level shows its definition), with
 * the occurrence proposed from the data and its basis.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type FmecaWorksheet,
  type OccurrenceProposal,
  saveReliabilityStudyFmecaItem,
} from "@/services/reliability-model-service";

import { RAMS_SELECT_CLASS, RamsCallout, useScaleLabel } from "./rams-ui";

/** A cause of the root-cause (Ishikawa) diagram turned into a new row: shown, and linked on save. */
export interface FmecaRootCauseLink {
  cause: string;
  diagramId: number;
  nodeId: string;
}

export interface FmecaDraft {
  itemId: number | null;
  rowVersion: number | null;
  failureModeId: number | null;
  componentLabel: string;
  functionalFailure: string;
  effect: string;
  mechanismId: number | null;
  causeId: number | null;
  severity: number | null;
  occurrence: number | null;
  detectability: number | null;
  currentControl: string;
  recommendedAction: string;
  occurrenceSource: "weibull" | "history" | "analyst";
  occurrenceEventsPerYear: number | null;
  proposal: OccurrenceProposal | null;
  /** New row from a root-cause diagram cause (no rating is preset: the analyst rates it). */
  rootCause: FmecaRootCauseLink | null;
}

export function FmecaItemDialog({
  open,
  onOpenChange,
  studyId,
  worksheet,
  draft,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  studyId: number;
  worksheet: FmecaWorksheet;
  draft: FmecaDraft;
  onSaved: (ws: FmecaWorksheet) => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal } = useFormatters();
  const scaleLabel = useScaleLabel();
  const [d, setD] = useState<FmecaDraft>(draft);
  const [saving, setSaving] = useState(false);
  useEffect(() => setD(draft), [draft]);

  const set = <K extends keyof FmecaDraft>(k: K, v: FmecaDraft[K]) =>
    setD((p) => ({ ...p, [k]: v }));
  const valid =
    d.failureModeId != null &&
    d.severity != null &&
    d.occurrence != null &&
    d.detectability != null;
  const rpn = valid ? (d.severity ?? 0) * (d.occurrence ?? 0) * (d.detectability ?? 0) : null;

  const submit = async () => {
    if (!valid || d.failureModeId == null) return;
    setSaving(true);
    try {
      onSaved(
        await saveReliabilityStudyFmecaItem({
          study_id: studyId,
          item_id: d.itemId,
          expected_row_version: d.rowVersion,
          failure_mode_id: d.failureModeId,
          component_label: d.componentLabel.trim() || null,
          functional_failure: d.functionalFailure.trim() || null,
          failure_effect: d.effect.trim() || null,
          failure_mechanism_id: d.mechanismId,
          failure_cause_id: d.causeId,
          severity: d.severity ?? 1,
          occurrence: d.occurrence ?? 1,
          detectability: d.detectability ?? 1,
          current_control: d.currentControl.trim() || null,
          recommended_action: d.recommendedAction.trim() || null,
          occurrence_source: d.occurrenceSource,
          occurrence_events_per_year:
            d.occurrenceSource === "analyst" ? null : d.occurrenceEventsPerYear,
          source_ram_ishikawa_diagram_id:
            d.itemId == null ? (d.rootCause?.diagramId ?? null) : null,
          source_ishikawa_flow_node_id: d.itemId == null ? (d.rootCause?.nodeId ?? null) : null,
        }),
      );
    } catch {
      // global IPC toast
    } finally {
      setSaving(false);
    }
  };

  const field = "grid gap-1 text-xs font-medium text-text-secondary";
  const rating = (
    kind: "severity" | "occurrence" | "detection",
    value: number | null,
    onChange: (v: number) => void,
  ) => (
    <label className={field}>
      {t(`rams.fmeca.${kind}`)}
      <select
        className={RAMS_SELECT_CLASS}
        value={value ?? ""}
        onChange={(e) => onChange(Number(e.target.value))}
      >
        <option value="" disabled>
          {t("rams.fmeca.choose")}
        </option>
        {[...worksheet.scales[kind]]
          .sort((a, b) => b.score - a.score)
          .map((l) => (
            <option key={l.score} value={l.score}>
              {`${l.score} — ${scaleLabel(l)}`}
            </option>
          ))}
      </select>
    </label>
  );
  const p = d.proposal;
  return (
    <Dialog open={open} onOpenChange={(o) => !saving && onOpenChange(o)}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>
            {d.itemId ? t("rams.fmeca.dialogEdit") : t("rams.fmeca.dialogAdd")}
          </DialogTitle>
          <DialogDescription>{t("rams.fmeca.dialogHint")}</DialogDescription>
        </DialogHeader>
        {d.rootCause ? (
          <RamsCallout tone="info" title={t("rams.fmeca.fromRootCause")}>
            {t("rams.fmeca.fromRootCauseBody", { cause: d.rootCause.cause })}
          </RamsCallout>
        ) : null}
        <div className="grid gap-3 sm:grid-cols-2">
          <label className={field}>
            {t("rams.fmeca.colElement")}
            <Input
              className="h-10"
              value={d.componentLabel}
              placeholder={t("rams.fmeca.elementPlaceholder")}
              onChange={(e) => set("componentLabel", e.target.value)}
            />
          </label>
          <label className={field}>
            {t("rams.fmeca.colMode")}
            <select
              className={RAMS_SELECT_CLASS}
              value={d.failureModeId ?? ""}
              disabled={d.itemId == null && d.proposal != null}
              onChange={(e) => set("failureModeId", Number(e.target.value))}
            >
              <option value="" disabled>
                {t("rams.fmeca.choose")}
              </option>
              {worksheet.modes.map((m) => (
                <option key={m.id} value={m.id}>{`${m.code} · ${m.label}`}</option>
              ))}
            </select>
          </label>
          <label className={field}>
            {t("rams.fmeca.mechanism")}
            <select
              className={RAMS_SELECT_CLASS}
              value={d.mechanismId ?? ""}
              onChange={(e) => set("mechanismId", e.target.value ? Number(e.target.value) : null)}
            >
              <option value="">{t("rams.fmeca.none")}</option>
              {worksheet.mechanisms.map((m) => (
                <option key={m.id} value={m.id}>{`${m.code} · ${m.label}`}</option>
              ))}
            </select>
          </label>
          <label className={field}>
            {t("rams.fmeca.cause")}
            <select
              className={RAMS_SELECT_CLASS}
              value={d.causeId ?? ""}
              onChange={(e) => set("causeId", e.target.value ? Number(e.target.value) : null)}
            >
              <option value="">{t("rams.fmeca.none")}</option>
              {worksheet.causes.map((m) => (
                <option key={m.id} value={m.id}>{`${m.code} · ${m.label}`}</option>
              ))}
            </select>
          </label>
          <label className={`${field} sm:col-span-2`}>
            {t("rams.fmeca.colEffect")}
            <Input
              className="h-10"
              value={d.effect}
              onChange={(e) => set("effect", e.target.value)}
            />
          </label>
          {rating("severity", d.severity, (v) => set("severity", v))}
          {rating("detection", d.detectability, (v) => set("detectability", v))}
          {rating("occurrence", d.occurrence, (v) => {
            set("occurrence", v);
            if (!p || v !== p.score) set("occurrenceSource", "analyst");
          })}
          <div className="self-end">
            {p ? (
              <RamsCallout
                tone="info"
                title={t("rams.fmeca.proposedO", { score: p.score })}
                action={
                  d.occurrence !== p.score || d.occurrenceSource === "analyst" ? (
                    <Button
                      variant="outline"
                      className="h-10"
                      onClick={() =>
                        setD((x) => ({
                          ...x,
                          occurrence: p.score,
                          occurrenceSource: p.source as FmecaDraft["occurrenceSource"],
                          occurrenceEventsPerYear: p.events_per_year,
                        }))
                      }
                    >
                      {t("rams.fmeca.useProposal")}
                    </Button>
                  ) : null
                }
              >
                {p.source === "weibull"
                  ? t("rams.fmeca.basisWeibull", {
                      rate: formatDecimal(p.events_per_year, 2),
                      mttf: formatDecimal(p.mean_life_hours ?? 0, 0),
                      hours: formatDecimal(p.hours_per_equipment_year ?? 0, 0),
                    })
                  : t("rams.fmeca.basisHistory", {
                      rate: formatDecimal(p.events_per_year, 2),
                      failures: p.failures,
                      years: formatDecimal(p.equipment_years, 1),
                    })}
              </RamsCallout>
            ) : (
              <p className="text-xs text-text-muted">{t("rams.fmeca.noProposal")}</p>
            )}
          </div>
          <label className={field}>
            {t("rams.fmeca.currentControl")}
            <Input
              className="h-10"
              value={d.currentControl}
              onChange={(e) => set("currentControl", e.target.value)}
            />
          </label>
          <label className={field}>
            {t("rams.fmeca.recommendedAction")}
            <Input
              className="h-10"
              value={d.recommendedAction}
              onChange={(e) => set("recommendedAction", e.target.value)}
            />
          </label>
        </div>
        <p className="text-sm text-text-secondary" aria-live="polite">
          {rpn != null ? t("rams.fmeca.rpnPreview", { rpn }) : t("rams.fmeca.rpnMissing")}
        </p>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {t("rams.common.cancel")}
          </Button>
          <Button className="h-11" disabled={!valid || saving} onClick={() => void submit()}>
            {t("rams.fmeca.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
