/**
 * Fault tree editor of a study: AND / OR gates and named basic events drawn as a tree; the top
 * event is the gate chosen by name. Results (top-event probability, minimal cut sets and
 * importance measures) are computed by the backend on every change, without saving.
 */
import { CircleDot, GitMerge, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import type { EquipmentLife, FtNode, FtResult, FtSpec } from "@/services/reliability-model-service";

import { SourceFields, defaultSource } from "./SourceFields";
import { newNodeId } from "./SystemRbdEditor";
import {
  ChartDataTable,
  RAMS_SELECT_CLASS,
  RAMS_TABLE_FRAME,
  RamsCallout,
  RamsCard,
} from "./rams-ui";

const NW = 132;
const NH = 46;
const HGAP = 16;
const VGAP = 46;

function width(id: string, by: Map<string, FtNode>, depth: number): number {
  const n = by.get(id);
  if (!n || n.node_type === "basic" || n.inputs.length === 0 || depth > 20) return NW;
  return Math.max(
    NW,
    n.inputs.reduce((s, c) => s + width(c, by, depth + 1), 0) + HGAP * (n.inputs.length - 1),
  );
}

function depthOf(id: string, by: Map<string, FtNode>, d: number): number {
  const n = by.get(id);
  if (!n || n.node_type === "basic" || d > 20) return d;
  return Math.max(d, ...n.inputs.map((c) => depthOf(c, by, d + 1)));
}

export function FtDiagram({
  spec,
  selected,
  onSelect,
  result,
  formatP,
  labels,
}: {
  spec: FtSpec;
  selected: string | null;
  onSelect: (id: string) => void;
  result: FtResult | null;
  formatP: (v: number) => string;
  labels: {
    title: string;
    and: string;
    or: string;
    select: (label: string, p: string) => string;
    top: string;
  };
}) {
  const by = new Map(spec.nodes.map((n) => [n.id, n]));
  const W = width(spec.top_id, by, 0) + 40;
  const H = (depthOf(spec.top_id, by, 0) + 1) * (NH + VGAP) + 20;
  const pOf = (id: string) =>
    id === spec.top_id
      ? (result?.top_probability ?? undefined)
      : result?.events.find((e) => e.id === id)?.probability;
  const parts: ReactNode[] = [];
  const draw = (id: string, x: number, y: number, depth: number, key: string) => {
    const n = by.get(id);
    if (!n || depth > 20) return;
    const w = width(id, by, depth);
    const cx = x + w / 2;
    const on = selected === id;
    const p = pOf(id);
    const gate = n.node_type !== "basic";
    parts.push(
      <g
        key={`${key}-n`}
        role="button"
        tabIndex={0}
        aria-pressed={on}
        aria-label={labels.select(n.label, p != null ? formatP(p) : "—")}
        onClick={() => onSelect(id)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect(id);
          }
        }}
        className="cursor-pointer"
      >
        <rect
          x={cx - NW / 2}
          y={y}
          width={NW}
          height={NH}
          rx={gate ? 4 : NH / 2}
          className={cn(
            on ? "fill-primary-bg stroke-primary" : "fill-surface-1 stroke-surface-border",
          )}
          strokeWidth={on || id === spec.top_id ? 2 : 1}
        />
        <text
          x={cx}
          y={y + 18}
          textAnchor="middle"
          className="fill-text-primary text-[12px] font-semibold"
        >
          {n.label.length > 18 ? `${n.label.slice(0, 17)}…` : n.label}
        </text>
        <text x={cx} y={y + 34} textAnchor="middle" className="fill-text-secondary text-[11px]">
          {gate
            ? `${n.node_type === "and" ? labels.and : labels.or}${p != null ? ` · ${formatP(p)}` : ""}`
            : p != null
              ? `p = ${formatP(p)}`
              : "—"}
        </text>
        {id === spec.top_id ? (
          <text
            x={cx}
            y={y - 6}
            textAnchor="middle"
            className="fill-primary text-[10px] font-semibold uppercase"
          >
            {labels.top}
          </text>
        ) : null}
      </g>,
    );
    if (!gate) return;
    let cxk = x;
    n.inputs.forEach((c, i) => {
      const cw = width(c, by, depth + 1);
      const childCx = cxk + cw / 2;
      parts.push(
        <path
          key={`${key}-e${i}`}
          d={`M${cx} ${y + NH} V${y + NH + VGAP / 2} H${childCx} V${y + NH + VGAP}`}
          fill="none"
          className="stroke-text-muted"
        />,
      );
      draw(c, cxk, y + NH + VGAP, depth + 1, `${key}.${i}`);
      cxk += cw + HGAP;
    });
  };
  draw(spec.top_id, 20, 18, 0, "t");
  return (
    <figure className="m-0 overflow-x-auto">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="group"
        aria-label={labels.title}
        style={{ minWidth: Math.min(W, 900) }}
        className="block h-auto w-full"
      >
        {parts}
      </svg>
      <ChartDataTable
        caption={labels.title}
        headers={["", "p"]}
        rows={spec.nodes.map((n) => {
          const p = pOf(n.id);
          return [n.label, p != null ? formatP(p) : "—"];
        })}
      />
    </figure>
  );
}

export function SystemFtaEditor({
  spec,
  onChange,
  selected,
  onSelect,
  result,
  equipment,
  fit,
  editable,
}: {
  spec: FtSpec;
  onChange: (s: FtSpec) => void;
  selected: string | null;
  onSelect: (id: string | null) => void;
  result: FtResult | null;
  equipment: EquipmentLife[];
  fit: [number, number] | null;
  editable: boolean;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal } = useFormatters();
  const formatP = (v: number) => (v > 0 && v < 0.001 ? v.toExponential(2) : formatDecimal(v, 4));
  const node = spec.nodes.find((n) => n.id === selected) ?? null;
  const gates = spec.nodes.filter((n) => n.node_type !== "basic");

  const patch = (id: string, p: Partial<FtNode>) =>
    onChange({ ...spec, nodes: spec.nodes.map((n) => (n.id === id ? { ...n, ...p } : n)) });

  const addInput = (parentId: string, type: FtNode["node_type"]) => {
    const id = newNodeId(type === "basic" ? "e" : "g");
    onChange({
      ...spec,
      nodes: [
        ...spec.nodes.map((n) => (n.id === parentId ? { ...n, inputs: [...n.inputs, id] } : n)),
        {
          id,
          label: type === "basic" ? t("rams.system.newEvent") : t("rams.system.newGate"),
          node_type: type,
          inputs: [],
          source:
            type === "basic" ? defaultSource(fit ? "study_fit" : "probability", equipment) : null,
        },
      ],
    });
    onSelect(id);
  };

  const linkExisting = (parentId: string, childId: string) =>
    onChange({
      ...spec,
      nodes: spec.nodes.map((n) =>
        n.id === parentId && !n.inputs.includes(childId)
          ? { ...n, inputs: [...n.inputs, childId] }
          : n,
      ),
    });

  const remove = (id: string) => {
    const drop = new Set<string>([id]);
    // Inputs used only under the removed node go with it.
    const walk = (x: string) =>
      spec.nodes
        .find((n) => n.id === x)
        ?.inputs.forEach((c) => {
          if (!spec.nodes.some((n) => !drop.has(n.id) && n.inputs.includes(c))) {
            drop.add(c);
            walk(c);
          }
        });
    walk(id);
    onChange({
      ...spec,
      nodes: spec.nodes
        .filter((n) => !drop.has(n.id))
        .map((n) => ({ ...n, inputs: n.inputs.filter((c) => !drop.has(c)) })),
    });
    onSelect(spec.top_id);
  };

  const issueText = (code: string, nodeId: string) =>
    t(`rams.system.issue.${code}`, { node: spec.nodes.find((n) => n.id === nodeId)?.label ?? "" });
  const reachable = new Set<string>();
  const mark = (x: string) => {
    if (reachable.has(x)) return;
    reachable.add(x);
    spec.nodes.find((n) => n.id === x)?.inputs.forEach(mark);
  };
  mark(spec.top_id);

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
      <RamsCard
        title={t("rams.system.ftaTitle", {
          top: spec.nodes.find((n) => n.id === spec.top_id)?.label ?? "",
        })}
        subtitle={t("rams.system.ftaHint")}
        actions={
          <label className="flex items-center gap-2 text-xs text-text-secondary">
            {t("rams.system.topEvent")}
            <select
              className={RAMS_SELECT_CLASS}
              value={spec.top_id}
              disabled={!editable}
              onChange={(e) => onChange({ ...spec, top_id: e.target.value })}
            >
              {gates.map((g) => (
                <option key={g.id} value={g.id}>
                  {g.label}
                </option>
              ))}
            </select>
          </label>
        }
      >
        <FtDiagram
          spec={spec}
          selected={selected}
          onSelect={onSelect}
          result={result}
          formatP={formatP}
          labels={{
            title: t("rams.system.ftaTitle", { top: "" }),
            and: t("rams.system.and"),
            or: t("rams.system.or"),
            top: t("rams.system.topEvent"),
            select: (label, p) => t("rams.system.selectEvent", { label, p }),
          }}
        />
      </RamsCard>
      <div className="grid content-start gap-4">
        <RamsCard
          title={
            node ? t("rams.system.nodeTitle", { label: node.label }) : t("rams.system.pickNode")
          }
        >
          {node ? (
            <div className="grid gap-3">
              <label className="grid gap-1 text-xs font-medium text-text-secondary">
                {t("rams.system.label")}
                <Input
                  className="h-10"
                  value={node.label}
                  disabled={!editable}
                  onChange={(e) => patch(node.id, { label: e.target.value })}
                />
              </label>
              {node.node_type === "basic" ? (
                <SourceFields
                  source={node.source}
                  kinds={[
                    "study_fit",
                    "equipment_history",
                    "probability",
                    "rate",
                    "weibull",
                    "exponential",
                  ]}
                  equipment={equipment}
                  fit={fit}
                  disabled={!editable}
                  onChange={(source) => patch(node.id, { source })}
                />
              ) : (
                <>
                  <div
                    className="inline-flex rounded-md border border-surface-border p-0.5"
                    role="group"
                    aria-label={t("rams.system.gate")}
                  >
                    {(["or", "and"] as const).map((k) => (
                      <button
                        key={k}
                        type="button"
                        aria-pressed={node.node_type === k}
                        disabled={!editable}
                        onClick={() => patch(node.id, { node_type: k })}
                        className={cn(
                          "min-h-9 rounded px-3 text-sm",
                          node.node_type === k
                            ? "bg-primary text-white"
                            : "text-text-secondary hover:bg-surface-2",
                        )}
                      >
                        {t(`rams.system.${k}Long`)}
                      </button>
                    ))}
                  </div>
                  {editable ? (
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => addInput(node.id, "basic")}
                      >
                        <CircleDot className="h-4 w-4" aria-hidden />
                        {t("rams.system.addEvent")}
                      </Button>
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => addInput(node.id, "or")}
                      >
                        <GitMerge className="h-4 w-4" aria-hidden />
                        {t("rams.system.addGate")}
                      </Button>
                      <label className="grid gap-1 text-xs font-medium text-text-secondary">
                        {t("rams.system.linkEvent")}
                        <select
                          className={RAMS_SELECT_CLASS}
                          value=""
                          onChange={(e) => e.target.value && linkExisting(node.id, e.target.value)}
                        >
                          <option value="">{t("rams.fmeca.choose")}</option>
                          {spec.nodes
                            .filter((n) => n.node_type === "basic" && !node.inputs.includes(n.id))
                            .map((n) => (
                              <option key={n.id} value={n.id}>
                                {n.label}
                              </option>
                            ))}
                        </select>
                      </label>
                    </div>
                  ) : null}
                </>
              )}
              {editable && node.id !== spec.top_id ? (
                <Button
                  variant="ghost"
                  className="h-10 justify-self-start"
                  onClick={() => remove(node.id)}
                >
                  <Trash2 className="h-4 w-4" aria-hidden />
                  {t("rams.system.remove")}
                </Button>
              ) : null}
              {!reachable.has(node.id) ? (
                <p className="text-xs text-text-muted">{t("rams.system.unreachable")}</p>
              ) : null}
            </div>
          ) : (
            <p className="text-sm text-text-muted">{t("rams.system.pickNodeHint")}</p>
          )}
        </RamsCard>
        <RamsCard
          title={t("rams.system.resultsTitle", {
            hours: formatDecimal(result?.mission_hours ?? 0, 0),
          })}
        >
          {result == null ? (
            <p className="text-sm text-text-muted">{t("rams.system.computing")}</p>
          ) : result.issues.length > 0 ? (
            <RamsCallout tone="warning" title={t("rams.system.incomplete")}>
              <ul className="list-disc ps-4">
                {result.issues.map((i, k) => (
                  <li key={k}>{issueText(i.code, i.node_id)}</li>
                ))}
              </ul>
            </RamsCallout>
          ) : (
            <>
              <KpiTile
                label={t("rams.system.topProbability")}
                value={formatP(result.top_probability ?? 0)}
                hint={t(
                  `rams.system.method.${result.probability_method ?? "exact_inclusion_exclusion"}`,
                )}
              />
              <h4 className="mt-3 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                {t("rams.system.cutSets")}
              </h4>
              <div className={`${RAMS_TABLE_FRAME} mt-1`}>
                <table className="w-full text-sm">
                  <thead>
                    <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                      <th className="px-3 py-2 text-start">{t("rams.system.colCutSet")}</th>
                      <th className="px-3 py-2 text-end">p</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.cut_sets.map((c, i) => (
                      <tr key={i} className="h-9 border-t border-surface-border">
                        <td className="px-3">{c.labels.join(" + ")}</td>
                        <td className="px-3 text-end font-mono">{formatP(c.probability)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <h4 className="mt-3 text-xs font-semibold uppercase tracking-wide text-text-secondary">
                {t("rams.system.importance")}
              </h4>
              <div className={`${RAMS_TABLE_FRAME} mt-1`}>
                <table className="w-full text-sm">
                  <thead>
                    <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                      <th className="px-3 py-2 text-start">{t("rams.system.colEvent")}</th>
                      <th className="px-3 py-2 text-end">p</th>
                      <th className="px-3 py-2 text-end" title={t("rams.system.fvHint")}>
                        FV
                      </th>
                      <th className="px-3 py-2 text-end" title={t("rams.system.birnbaumHint")}>
                        {t("rams.system.colBirnbaum")}
                      </th>
                      <th className="px-3 py-2 text-end" title={t("rams.system.rawHint")}>
                        RAW
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.events.map((e) => (
                      <tr key={e.id} className="h-9 border-t border-surface-border">
                        <td className="px-3">{e.label}</td>
                        <td className="px-3 text-end font-mono">{formatP(e.probability)}</td>
                        <td className="px-3 text-end font-mono">
                          {formatDecimal(e.fussell_vesely * 100, 0)} %
                        </td>
                        <td className="px-3 text-end font-mono">{formatP(e.birnbaum)}</td>
                        <td className="px-3 text-end font-mono">
                          {formatDecimal(e.risk_achievement_worth, 1)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </RamsCard>
      </div>
    </div>
  );
}
