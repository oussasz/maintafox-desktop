/**
 * Reliability block diagram editor of a study: series / parallel (k-out-of-n) structures and
 * blocks drawn as a diagram; select a block or a structure to edit it. Results (system
 * reliability, sub-structures, importance and share of the unreliability) are computed by the
 * backend on every change, without saving.
 */
import { GitBranch, Rows3, Square, Trash2 } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import type {
  EquipmentLife,
  RbdNode,
  RbdResult,
  RbdSpec,
} from "@/services/reliability-model-service";

import { SourceFields, defaultSource } from "./SourceFields";
import { ChartDataTable, RAMS_TABLE_FRAME, RamsCallout, RamsCard } from "./rams-ui";

const BW = 136;
const BH = 50;
const GAP = 22;
const RAIL = 22;

interface Box {
  w: number;
  h: number;
}

function measure(id: string, by: Map<string, RbdNode>, depth = 0): Box {
  const n = by.get(id);
  if (!n || depth > 30) return { w: BW, h: BH };
  if (n.node_type === "block") return { w: BW, h: BH };
  const kids = n.children.map((c) => measure(c, by, depth + 1));
  if (kids.length === 0) return { w: BW, h: BH };
  if (n.node_type === "series") {
    return {
      w: kids.reduce((s, k) => s + k.w, 0) + GAP * (kids.length - 1),
      h: Math.max(...kids.map((k) => k.h)),
    };
  }
  return {
    w: Math.max(...kids.map((k) => k.w)) + 2 * RAIL,
    h: kids.reduce((s, k) => s + k.h, 0) + GAP * (kids.length - 1),
  };
}

let uid = 0;
export function newNodeId(prefix: string): string {
  uid += 1;
  return `${prefix}_${Date.now().toString(36)}_${uid}`;
}

export function RbdDiagram({
  spec,
  selected,
  onSelect,
  result,
  formatR,
  labels,
}: {
  spec: RbdSpec;
  selected: string | null;
  onSelect: (id: string) => void;
  result: RbdResult | null;
  formatR: (v: number) => string;
  labels: {
    title: string;
    kOfN: (k: number, n: number) => string;
    select: (label: string, r: string) => string;
  };
}) {
  const by = new Map(spec.nodes.map((n) => [n.id, n]));
  const box = measure(spec.root_id, by);
  const W = box.w + 60;
  const H = box.h + 50;
  const rOf = (id: string) => result?.nodes.find((n) => n.id === id)?.reliability;
  const parts: ReactNode[] = [];
  const draw = (id: string, x: number, y: number, depth: number): Box => {
    const n = by.get(id);
    const b = measure(id, by);
    if (!n || depth > 30) return b;
    const midY = y + b.h / 2;
    if (n.node_type === "block" || n.children.length === 0) {
      const r = rOf(id);
      const on = selected === id;
      parts.push(
        <g
          key={id}
          role="button"
          tabIndex={0}
          aria-label={labels.select(n.label, r != null ? formatR(r) : "—")}
          aria-pressed={on}
          onClick={() => onSelect(id)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onSelect(id);
            }
          }}
          className="cursor-pointer focus:outline-none [&:focus-visible>rect]:stroke-[3px]"
        >
          <rect
            x={x}
            y={midY - BH / 2}
            width={BW}
            height={BH}
            rx={6}
            className={cn(
              on ? "fill-primary-bg stroke-primary" : "fill-surface-1 stroke-surface-border",
            )}
            strokeWidth={on ? 2 : 1}
            strokeDasharray={n.node_type === "block" ? undefined : "4 3"}
          />
          <text
            x={x + BW / 2}
            y={midY - 4}
            textAnchor="middle"
            className="fill-text-primary text-[12px] font-semibold"
          >
            {n.label.length > 18 ? `${n.label.slice(0, 17)}…` : n.label}
          </text>
          <text
            x={x + BW / 2}
            y={midY + 13}
            textAnchor="middle"
            className="fill-text-secondary text-[11px]"
          >
            {r != null ? `R = ${formatR(r)}` : "—"}
          </text>
        </g>,
      );
      return b;
    }
    const kids = n.children.map((c) => ({ id: c, b: measure(c, by) }));
    if (n.node_type === "series") {
      let cx = x;
      kids.forEach((k, i) => {
        const ky = y + (b.h - k.b.h) / 2;
        draw(k.id, cx, ky, depth + 1);
        if (i > 0) {
          parts.push(
            <line
              key={`${id}-s${i}`}
              x1={cx - GAP}
              x2={cx}
              y1={midY}
              y2={midY}
              className="stroke-text-muted"
            />,
          );
        }
        cx += k.b.w + GAP;
      });
    } else {
      let cy = y;
      const left = x;
      const right = x + b.w;
      kids.forEach((k) => {
        const kx = x + RAIL + (b.w - 2 * RAIL - k.b.w) / 2;
        draw(k.id, kx, cy, depth + 1);
        const ky = cy + k.b.h / 2;
        parts.push(
          <g key={`${id}-p${k.id}`} className="stroke-text-muted" fill="none">
            <path d={`M${left} ${ky} H${kx}`} />
            <path d={`M${kx + k.b.w} ${ky} H${right}`} />
          </g>,
        );
        cy += k.b.h + GAP;
      });
      const first = y + (kids[0]?.b.h ?? 0) / 2;
      const last = cy - GAP - (kids[kids.length - 1]?.b.h ?? 0) / 2;
      parts.push(
        <g key={`${id}-rails`} className="stroke-text-muted">
          <line x1={left} x2={left} y1={first} y2={last} />
          <line x1={right} x2={right} y1={first} y2={last} />
        </g>,
      );
      const k = n.k ?? 1;
      parts.push(
        <g
          key={`${id}-label`}
          role="button"
          tabIndex={0}
          aria-pressed={selected === id}
          aria-label={labels.select(n.label, rOf(id) != null ? formatR(rOf(id) ?? 0) : "—")}
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
            x={right - 30}
            y={midY - 10}
            width={60}
            height={20}
            rx={10}
            className={cn(
              selected === id ? "fill-primary" : "fill-surface-2",
              "stroke-surface-border",
            )}
          />
          <text
            x={right}
            y={midY + 4}
            textAnchor="middle"
            className={cn("text-[10px]", selected === id ? "fill-white" : "fill-text-secondary")}
          >
            {labels.kOfN(k, n.children.length)}
          </text>
        </g>,
      );
    }
    if (n.node_type === "series" && selected === id) {
      parts.push(
        <rect
          key={`${id}-sel`}
          x={x - 6}
          y={y - 6}
          width={b.w + 12}
          height={b.h + 12}
          rx={8}
          fill="none"
          className="stroke-primary"
          strokeDasharray="5 4"
        />,
      );
    }
    return b;
  };
  draw(spec.root_id, 30, 25, 0);
  return (
    <figure className="m-0 overflow-x-auto">
      <svg
        viewBox={`0 0 ${W} ${H}`}
        role="group"
        aria-label={labels.title}
        style={{ minWidth: Math.min(W, 900) }}
        className="block h-auto w-full"
      >
        <line
          x1={6}
          x2={30}
          y1={25 + box.h / 2}
          y2={25 + box.h / 2}
          className="stroke-text-muted"
        />
        <line
          x1={30 + box.w}
          x2={W - 6}
          y1={25 + box.h / 2}
          y2={25 + box.h / 2}
          className="stroke-text-muted"
        />
        {parts}
      </svg>
      <ChartDataTable
        caption={labels.title}
        headers={["", "R"]}
        rows={spec.nodes.map((n) => [n.label, rOf(n.id) != null ? formatR(rOf(n.id) ?? 0) : "—"])}
      />
    </figure>
  );
}

export function SystemRbdEditor({
  spec,
  onChange,
  selected,
  onSelect,
  result,
  evalError,
  equipment,
  fit,
  editable,
}: {
  spec: RbdSpec;
  onChange: (s: RbdSpec) => void;
  selected: string | null;
  onSelect: (id: string | null) => void;
  result: RbdResult | null;
  /** Why the model could not be evaluated (shown instead of "computing…"). */
  evalError: unknown;
  equipment: EquipmentLife[];
  fit: [number, number] | null;
  editable: boolean;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal } = useFormatters();
  const formatR = (v: number) => formatDecimal(v, 3);
  const node = spec.nodes.find((n) => n.id === selected) ?? null;
  const parentOf = (id: string) => spec.nodes.find((n) => n.children.includes(id));

  const patch = (id: string, p: Partial<RbdNode>) =>
    onChange({ ...spec, nodes: spec.nodes.map((n) => (n.id === id ? { ...n, ...p } : n)) });

  const addChild = (parentId: string, type: RbdNode["node_type"]) => {
    const id = newNodeId(type === "block" ? "b" : "s");
    const child: RbdNode = {
      id,
      label: type === "block" ? t("rams.system.newBlock") : t(`rams.system.new_${type}`),
      node_type: type,
      children: [],
      k: type === "parallel" ? 1 : null,
      source:
        type === "block" ? defaultSource(fit ? "study_fit" : "equipment_history", equipment) : null,
      group: null,
      mttr_hours: null,
    };
    onChange({
      ...spec,
      nodes: [
        ...spec.nodes.map((n) => (n.id === parentId ? { ...n, children: [...n.children, id] } : n)),
        child,
      ],
    });
    onSelect(id);
  };

  const remove = (id: string) => {
    const drop = new Set<string>();
    const walk = (x: string) => {
      drop.add(x);
      spec.nodes.find((n) => n.id === x)?.children.forEach(walk);
    };
    walk(id);
    onChange({
      ...spec,
      nodes: spec.nodes
        .filter((n) => !drop.has(n.id))
        .map((n) => ({ ...n, children: n.children.filter((c) => !drop.has(c)) })),
    });
    onSelect(parentOf(id)?.id ?? spec.root_id);
  };

  const issueText = (code: string, nodeId: string) =>
    t(`rams.system.issue.${code}`, { node: spec.nodes.find((n) => n.id === nodeId)?.label ?? "" });

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
      <RamsCard title={t("rams.system.rbdTitle")} subtitle={t("rams.system.rbdHint")}>
        <RbdDiagram
          spec={spec}
          selected={selected}
          onSelect={onSelect}
          result={result}
          formatR={formatR}
          labels={{
            title: t("rams.system.rbdTitle"),
            kOfN: (k, n) => t("rams.system.kOfN", { k, n }),
            select: (label, r) => t("rams.system.selectBlock", { label, r }),
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
            // Keyed by node: the number fields keep their own text while typing, so another
            // node must start from its own values (they showed the previous node's).
            <div key={node.id} className="grid gap-3">
              <label className="grid gap-1 text-xs font-medium text-text-secondary">
                {t("rams.system.label")}
                <Input
                  className="h-10"
                  value={node.label}
                  disabled={!editable}
                  onChange={(e) => patch(node.id, { label: e.target.value })}
                />
              </label>
              {node.node_type === "block" ? (
                <>
                  <SourceFields
                    source={node.source}
                    kinds={[
                      "study_fit",
                      "equipment_history",
                      "weibull",
                      "exponential",
                      "reliability",
                    ]}
                    equipment={equipment}
                    fit={fit}
                    disabled={!editable}
                    onChange={(source) => patch(node.id, { source })}
                  />
                  <div className="grid grid-cols-2 gap-2">
                    <label className="grid gap-1 text-xs font-medium text-text-secondary">
                      {t("rams.system.group")}
                      <Input
                        className="h-10"
                        value={node.group ?? ""}
                        disabled={!editable}
                        onChange={(e) => patch(node.id, { group: e.target.value || null })}
                      />
                    </label>
                    <label className="grid gap-1 text-xs font-medium text-text-secondary">
                      {t("rams.system.mttr")}
                      <Input
                        className="h-10"
                        inputMode="decimal"
                        defaultValue={node.mttr_hours != null ? String(node.mttr_hours) : ""}
                        disabled={!editable}
                        onChange={(e) => {
                          const v = Number(e.target.value.replace(",", "."));
                          patch(node.id, {
                            mttr_hours:
                              e.target.value.trim() && Number.isFinite(v) && v > 0 ? v : null,
                          });
                        }}
                      />
                    </label>
                  </div>
                </>
              ) : (
                <>
                  <div
                    className="inline-flex rounded-md border border-surface-border p-0.5"
                    role="group"
                    aria-label={t("rams.system.structure")}
                  >
                    {(["series", "parallel"] as const).map((k) => (
                      <button
                        key={k}
                        type="button"
                        aria-pressed={node.node_type === k}
                        disabled={!editable}
                        onClick={() =>
                          patch(node.id, {
                            node_type: k,
                            k: k === "parallel" ? (node.k ?? 1) : null,
                          })
                        }
                        className={cn(
                          "min-h-9 rounded px-3 text-sm",
                          node.node_type === k
                            ? "bg-primary text-white"
                            : "text-text-secondary hover:bg-surface-2",
                        )}
                      >
                        {t(`rams.system.${k}`)}
                      </button>
                    ))}
                  </div>
                  {node.node_type === "parallel" ? (
                    <label className="grid max-w-[12rem] gap-1 text-xs font-medium text-text-secondary">
                      {t("rams.system.kLabel", { n: node.children.length })}
                      <Input
                        className="h-10"
                        type="number"
                        min={1}
                        max={Math.max(1, node.children.length)}
                        value={node.k ?? 1}
                        disabled={!editable}
                        onChange={(e) =>
                          patch(node.id, { k: Math.max(1, Number(e.target.value) || 1) })
                        }
                      />
                    </label>
                  ) : null}
                  {editable ? (
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => addChild(node.id, "block")}
                      >
                        <Square className="h-4 w-4" aria-hidden />
                        {t("rams.system.addBlock")}
                      </Button>
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => addChild(node.id, "series")}
                      >
                        <Rows3 className="h-4 w-4 rotate-90" aria-hidden />
                        {t("rams.system.addSeries")}
                      </Button>
                      <Button
                        variant="outline"
                        className="h-10"
                        onClick={() => addChild(node.id, "parallel")}
                      >
                        <GitBranch className="h-4 w-4" aria-hidden />
                        {t("rams.system.addParallel")}
                      </Button>
                    </div>
                  ) : null}
                </>
              )}
              {editable && node.id !== spec.root_id ? (
                <Button
                  variant="ghost"
                  className="h-10 justify-self-start"
                  onClick={() => remove(node.id)}
                >
                  <Trash2 className="h-4 w-4" aria-hidden />
                  {t("rams.system.remove")}
                </Button>
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
          {evalError != null ? (
            <RamsCallout tone="danger" title={t("rams.system.evalError")}>
              {describeAppError(evalError).title}
            </RamsCallout>
          ) : result == null ? (
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
              <div className="grid grid-cols-2 gap-3">
                <KpiTile
                  label={t("rams.system.systemR")}
                  value={formatR(result.system_reliability ?? 0)}
                  hint={t("rams.system.systemRHint")}
                />
                {spec.nodes
                  .find((n) => n.id === spec.root_id)
                  ?.children.slice(0, 1)
                  .map((c) => {
                    const r = result.nodes.find((x) => x.id === c);
                    return r ? (
                      <KpiTile
                        key={c}
                        label={r.label}
                        value={formatR(r.reliability)}
                        hint={t("rams.system.branchHint")}
                      />
                    ) : null;
                  })}
              </div>
              <div className={`${RAMS_TABLE_FRAME} mt-3`}>
                <table className="w-full text-sm">
                  <thead>
                    <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                      <th className="px-3 py-2 text-start">{t("rams.system.colGroup")}</th>
                      <th className="px-3 py-2 text-end">{t("rams.system.colShare")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.groups.map((g) => (
                      <tr key={g.group} className="h-9 border-t border-surface-border">
                        <td className="px-3">{g.group}</td>
                        <td className="px-3 text-end font-mono">
                          {formatDecimal(g.share * 100, 0)} %
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <div className={`${RAMS_TABLE_FRAME} mt-3`}>
                <table className="w-full text-sm">
                  <thead>
                    <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                      <th className="px-3 py-2 text-start">{t("rams.system.colBlock")}</th>
                      <th className="px-3 py-2 text-end">R</th>
                      <th className="px-3 py-2 text-end" title={t("rams.system.birnbaumHint")}>
                        {t("rams.system.colBirnbaum")}
                      </th>
                      <th className="px-3 py-2 text-end" title={t("rams.system.criticalityHint")}>
                        {t("rams.system.colCriticality")}
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {result.nodes
                      .filter((n) => n.node_type === "block")
                      .map((n) => (
                        <tr key={n.id} className="h-9 border-t border-surface-border">
                          <td className="px-3">{n.label}</td>
                          <td className="px-3 text-end font-mono">{formatR(n.reliability)}</td>
                          <td className="px-3 text-end font-mono">
                            {formatDecimal(n.birnbaum ?? 0, 3)}
                          </td>
                          <td className="px-3 text-end font-mono">
                            {formatDecimal((n.criticality ?? 0) * 100, 0)} %
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
