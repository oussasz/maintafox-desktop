/**
 * Printable and CSV report of a reliability study, or of its decision note for management
 * (L2-14). One model feeds both outputs, so the print, the PDF (printed to PDF from the print
 * dialog) and the CSV say the same thing. Pure: the caller passes the data and the formatters.
 */
import type { CsvCell } from "@/export/csv";
import {
  DOCUMENT_HEADER_PRINT_CSS,
  wrapDocumentHeaderBlock,
} from "@/export/templates/document-header";
import type {
  LifeAnalysis,
  ReliabilityStudy,
  StudyDecision,
} from "@/services/reliability-study-service";

import { roundInterval } from "./rams-ui";

export type StudyReportScope = "study" | "note";

export interface StudyReportFormat {
  t: (key: string, opts?: Record<string, unknown>) => string;
  number: (v: number) => string;
  decimal: (v: number, digits: number) => string;
  /** Money in the currency of the study costs (else the plant currency). */
  money: (v: number) => string;
  date: (v: string) => string;
}

export interface StudyReportSection {
  title: string;
  /** Sentences shown before the rows. */
  text: string[];
  rows: [string, string][];
}

export interface StudyReport {
  title: string;
  sections: StudyReportSection[];
}

/** "Label : value" with the spacing of the language. */
function labelled(f: StudyReportFormat, label: string, value: string): string {
  return f.t("rams.export.labelled", { label, value });
}

function scopeSection(
  study: ReliabilityStudy,
  analysis: LifeAnalysis | null,
  f: StudyReportFormat,
): StudyReportSection {
  const { t } = f;
  const basis = analysis?.effective_time_basis ?? null;
  return {
    title: t("rams.export.scope"),
    text: [],
    rows: [
      [t("rams.studyName.name"), study.title],
      [
        t("rams.export.equipment"),
        study.assets.map((a) => `${a.asset_code} · ${a.asset_name}`).join(", "),
      ],
      [
        t("rams.export.period"),
        t("rams.export.periodValue", {
          from: f.date(study.period_start),
          to: f.date(study.period_end),
        }),
      ],
      [
        t("rams.export.mode"),
        study.failure_mode_label
          ? `${study.failure_mode_code ?? ""} · ${study.failure_mode_label}`
          : t("rams.study.allModes"),
      ],
      ...(basis
        ? ([
            [
              t("rams.export.basis"),
              basis === "operating"
                ? t("rams.study.basisUsedOperating")
                : t("rams.study.basisUsedCalendar"),
            ],
          ] as [string, string][])
        : []),
      ...(study.created_by_name
        ? ([[t("rams.export.author"), study.created_by_name]] as [string, string][])
        : []),
    ],
  };
}

function lifeSection(analysis: LifeAnalysis, f: StudyReportFormat): StudyReportSection {
  const { t } = f;
  const fit = analysis.fit;
  const hours = (v: number) => f.number(Math.round(v));
  if (!fit) {
    const issue = analysis.fit_issue ?? "not_enough_failures";
    return {
      title: t("rams.export.lifeTitle"),
      text: [
        labelled(f, t("rams.export.noFit"), t(`rams.life.issue.${issue}.title`)),
        t(`rams.life.issue.${issue}.body`, {
          min: analysis.min_failures,
          count: analysis.n_failures,
        }),
      ],
      rows: [],
    };
  }
  const bounds = (lo: string, hi: string) => t("rams.life.bounds", { lo, hi });
  return {
    title: t("rams.export.lifeTitle"),
    text: [
      labelled(
        f,
        t("rams.export.reading"),
        `${t(`rams.life.pattern.${fit.pattern}.title`)}. ${t(`rams.life.pattern.${fit.pattern}.body`)}`,
      ),
    ],
    rows: [
      [
        t("rams.life.beta"),
        `${f.decimal(fit.beta, 2)} (${bounds(f.decimal(fit.beta_lower, 2), f.decimal(fit.beta_upper, 2))})`,
      ],
      [
        t("rams.life.eta"),
        `${hours(fit.eta)} h (${bounds(hours(fit.eta_lower), hours(fit.eta_upper))})`,
      ],
      [t("rams.life.b10"), `${hours(fit.b10)} h`],
      [t("rams.life.mttf"), `${hours(fit.mean_life)} h`],
      [
        t("rams.life.data"),
        `${f.number(analysis.n_failures)} ${t("rams.life.failuresUnit")} · ${t(
          "rams.life.suspensions",
          { count: analysis.n_suspensions },
        )}`,
      ],
      ...(fit.rho != null
        ? ([[t("rams.life.rho"), f.decimal(fit.rho, 3)]] as [string, string][])
        : []),
    ],
  };
}

function noteSection(
  study: ReliabilityStudy,
  analysis: LifeAnalysis | null,
  f: StudyReportFormat,
): StudyReportSection {
  const { t } = f;
  const section: StudyReportSection = { title: t("rams.memo.title"), text: [], rows: [] };
  if (!analysis?.fit) {
    section.text.push(t("rams.export.noFit"));
    return section;
  }
  const cost = analysis.cost;
  if (!cost) {
    section.text.push(t("rams.export.noCosts"));
    return section;
  }
  const unit =
    analysis.effective_time_basis === "operating"
      ? t("rams.units.opHours")
      : t("rams.units.calHours");
  const plan = analysis.pm_plans.find((p) => p.pm_plan_id === analysis.selected_pm_plan_id) ?? null;
  const current = plan?.interval_hours ?? null;
  const hours = (v: number | null | undefined) => (v == null ? "—" : f.number(Math.round(v)));
  const money = (v: number | null | undefined) => (v == null ? "—" : f.money(v));
  const proposed = cost.proposed?.interval != null ? roundInterval(cost.proposed.interval) : null;
  if (proposed != null) {
    section.text.push(
      `${t(current != null ? "rams.memo.proposeVsCurrent" : "rams.memo.propose", {
        assets: study.assets.map((a) => a.asset_code).join(", "),
        interval: f.number(proposed),
        current: hours(current),
        unit,
      })} ${t("rams.memo.assumptions", {
        planned: money(cost.planned_cost),
        failure: money(cost.failure_cost),
        hpy: hours(analysis.operating_hours_per_year),
        unit,
        count: analysis.asset_count,
      })}`,
    );
  } else {
    section.text.push(t(`rams.memo.noInterval.${cost.rejection ?? "no_benefit"}`));
  }
  const strategies: [string, { cost_rate: number; annual_cost: number | null }][] = [
    [t("rams.memo.rtf"), cost.run_to_failure],
  ];
  if (cost.current)
    strategies.push([
      t("rams.memo.current", { interval: hours(cost.current.interval) }),
      cost.current,
    ]);
  if (cost.proposed && proposed != null)
    strategies.push([t("rams.memo.proposed", { interval: f.number(proposed) }), cost.proposed]);
  for (const [label, s] of strategies) {
    section.rows.push([
      label,
      `${labelled(f, t("rams.memo.colRate", { unit }), money(s.cost_rate))} · ${labelled(
        f,
        t("rams.memo.colAnnual", { count: analysis.asset_count }),
        money(s.annual_cost),
      )}`,
    ]);
  }
  section.rows.push([t("rams.export.plannedCost"), money(cost.planned_cost)]);
  section.rows.push([t("rams.export.failureCost"), money(cost.failure_cost)]);
  return section;
}

/** Status of a decision in words; a proposer's own rejection is a withdrawal. */
function statusKey(d: StudyDecision): string {
  return d.status === "rejected" && d.proposed_by_id != null && d.decided_by_id === d.proposed_by_id
    ? "withdrawn"
    : d.status;
}

function decisionsSection(decisions: StudyDecision[], f: StudyReportFormat): StudyReportSection {
  const { t } = f;
  return {
    title: t("rams.decisions.title"),
    text: decisions.length === 0 ? [t("rams.export.noDecisions")] : [],
    rows: decisions.map((d) => {
      const parts = [t(`rams.decision.kinds.${d.action_kind}`)];
      if (d.action_kind === "pm_interval" && d.proposed_trigger_interval_value != null)
        parts.push(
          t("rams.decisions.newInterval", {
            value: f.number(d.proposed_trigger_interval_value),
            unit: t(`rams.units.interval.${d.proposed_trigger_interval_unit ?? "operating_hours"}`),
          }),
        );
      if (d.asset_code) parts.push(d.asset_code);
      parts.push(t(`rams.decisions.status.${statusKey(d)}`));
      parts.push(
        t("rams.decisions.proposedBy", {
          name: d.proposed_by_name ?? "—",
          date: f.date(d.proposed_at),
        }),
      );
      parts.push(
        d.approver_name
          ? t("rams.decisions.approver", { name: d.approver_name })
          : t("rams.decisions.approverAny"),
      );
      if (d.decided_by_name && d.decided_at && d.status !== "proposed")
        parts.push(
          t(`rams.decisions.decidedBy.${statusKey(d)}`, {
            name: d.decided_by_name,
            date: f.date(d.decided_at),
          }),
        );
      if (d.justification)
        parts.push(t("rams.decisions.justificationText", { text: d.justification }));
      if (d.decision_note) parts.push(d.decision_note);
      return [d.title, parts.join(" · ")];
    }),
  };
}

/** The report of a whole study, or of its decision note only. */
export function buildStudyReport(
  scope: StudyReportScope,
  study: ReliabilityStudy,
  analysis: LifeAnalysis | null,
  decisions: StudyDecision[],
  f: StudyReportFormat,
): StudyReport {
  const sections = [scopeSection(study, analysis, f)];
  if (scope === "study" && analysis) sections.push(lifeSection(analysis, f));
  sections.push(noteSection(study, analysis, f));
  if (scope === "study") sections.push(decisionsSection(decisions, f));
  return {
    title:
      scope === "study"
        ? f.t("rams.export.studyTitle", { title: study.title })
        : f.t("rams.export.noteTitle", { title: study.title }),
    sections,
  };
}

/** CSV lines: section, item, value (sentences as lines with an empty item). */
export function studyReportCsvRows(report: StudyReport): CsvCell[][] {
  const rows: CsvCell[][] = [];
  for (const s of report.sections) {
    for (const line of s.text) rows.push([s.title, "", line]);
    for (const [item, value] of s.rows) rows.push([s.title, item, value]);
  }
  return rows;
}

function esc(v: string): string {
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Self-contained A4 document for the print transport (letterhead of the plant on top). */
export function studyReportHtml(
  report: StudyReport,
  letterheadHtml: string,
  printedAt: string,
  lang: string,
): string {
  const sections = report.sections
    .map(
      (s) => `<section>
  <h2>${esc(s.title)}</h2>
  ${s.text.map((p) => `<p>${esc(p)}</p>`).join("\n  ")}
  ${
    s.rows.length > 0
      ? `<table><tbody>${s.rows
          .map(([k, v]) => `<tr><th>${esc(k)}</th><td>${esc(v)}</td></tr>`)
          .join("")}</tbody></table>`
      : ""
  }
</section>`,
    )
    .join("\n");
  return `<!DOCTYPE html>
<html lang="${esc(lang)}">
<head>
  <meta charset="utf-8" />
  <title>${esc(report.title)}</title>
  <style>
    @page { size: A4; margin: 15mm; }
    body { font-family: Arial, sans-serif; margin: 0; padding: 16px 24px; font-size: 11px; color: #222; }
    h1 { font-size: 16px; margin: 0 0 2px; }
    .printed { color: #666; font-size: 10px; margin: 0 0 14px; }
    section { margin-bottom: 14px; page-break-inside: avoid; }
    h2 { font-size: 12px; background: #eee; padding: 4px 8px; margin: 0 0 6px; border-left: 3px solid #555; }
    p { margin: 0 0 6px; line-height: 1.4; }
    table { width: 100%; border-collapse: collapse; }
    th, td { border: 1px solid #ccc; padding: 4px 6px; text-align: left; vertical-align: top; font-size: 10px; }
    th { width: 32%; background: #f6f6f6; font-weight: 600; }
    ${DOCUMENT_HEADER_PRINT_CSS}
  </style>
</head>
<body>
  ${wrapDocumentHeaderBlock(letterheadHtml)}
  <h1>${esc(report.title)}</h1>
  <p class="printed">${esc(printedAt)}</p>
  ${sections}
</body>
</html>`;
}
