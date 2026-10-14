/**
 * Advanced tools of the system model (specialists), with guided forms and no JSON:
 * Markov chain (states and rates), Monte Carlo simulation of the study block diagram, event tree.
 * Every result is computed on request ("Calculer"); nothing is written.
 */
import { Play, Plus, Trash2 } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type EquipmentLife,
  type EventTreeResult,
  type MarkovInput,
  type MarkovResult,
  type RbdSpec,
  type SimulationResult,
  evaluateReliabilityEventTree,
  evaluateReliabilityMarkov,
  simulateReliabilityStudySystem,
} from "@/services/reliability-model-service";

import { newNodeId } from "./SystemRbdEditor";
import { RAMS_SELECT_CLASS, RAMS_TABLE_FRAME, RamsCallout, RamsCard } from "./rams-ui";

type Tool = "markov" | "simulation" | "event_tree";

const num = (v: string): number => (v.trim() === "" ? Number.NaN : Number(v.replace(",", ".")));

export function AdvancedTools({
  studyId,
  rbd,
  missionHours,
  equipment,
}: {
  studyId: number;
  rbd: RbdSpec | null;
  missionHours: number | null;
  equipment: EquipmentLife[];
}) {
  const { t } = useTranslation("reliability");
  const [tool, setTool] = useState<Tool>("markov");
  return (
    <div className="grid gap-4">
      <div
        className="inline-flex w-fit rounded-md border border-surface-border bg-surface-1 p-0.5"
        role="group"
        aria-label={t("rams.advanced.tools")}
      >
        {(["markov", "simulation", "event_tree"] as Tool[]).map((k) => (
          <button
            key={k}
            type="button"
            aria-pressed={tool === k}
            onClick={() => setTool(k)}
            className={cn(
              "min-h-9 rounded px-3 text-sm",
              tool === k ? "bg-primary text-white" : "text-text-secondary hover:bg-surface-2",
            )}
          >
            {t(`rams.advanced.${k}.title`)}
          </button>
        ))}
      </div>
      {tool === "markov" ? <MarkovTool equipment={equipment} /> : null}
      {tool === "simulation" ? (
        <SimulationTool studyId={studyId} rbd={rbd} missionHours={missionHours} />
      ) : null}
      {tool === "event_tree" ? <EventTreeTool /> : null}
    </div>
  );
}

interface StateRow {
  id: string;
  label: string;
  up: boolean;
}
interface TransitionRow {
  key: string;
  from: string;
  to: string;
  rate: string;
}

function MarkovTool({ equipment }: { equipment: EquipmentLife[] }) {
  const { t } = useTranslation("reliability");
  const { formatDecimal, formatNumber } = useFormatters();
  const [states, setStates] = useState<StateRow[]>([]);
  const [transitions, setTransitions] = useState<TransitionRow[]>([]);
  const [result, setResult] = useState<MarkovResult | null>(null);
  const [busy, setBusy] = useState(false);

  const fromEquipment = (eq: EquipmentLife) => {
    if (eq.mtbf_hours == null || eq.mttr_hours == null || eq.mttr_hours <= 0) return;
    const up = newNodeId("m");
    const down = newNodeId("m");
    setStates([
      { id: up, label: t("rams.advanced.markov.running", { code: eq.asset_code }), up: true },
      { id: down, label: t("rams.advanced.markov.failed", { code: eq.asset_code }), up: false },
    ]);
    setTransitions([
      { key: newNodeId("t"), from: up, to: down, rate: (1 / eq.mtbf_hours).toPrecision(4) },
      { key: newNodeId("t"), from: down, to: up, rate: (1 / eq.mttr_hours).toPrecision(4) },
    ]);
    setResult(null);
  };

  const input: MarkovInput = {
    states: states.map((s) => ({ id: s.id, label: s.label, up: s.up })),
    transitions: transitions.map((x) => ({ from: x.from, to: x.to, rate_per_hour: num(x.rate) })),
  };
  const valid =
    states.length >= 2 &&
    states.some((s) => s.up) &&
    states.some((s) => !s.up) &&
    transitions.length > 0 &&
    input.transitions.every(
      (x) => Number.isFinite(x.rate_per_hour) && x.rate_per_hour > 0 && x.from !== x.to,
    );

  return (
    <RamsCard title={t("rams.advanced.markov.title")} subtitle={t("rams.advanced.markov.hint")}>
      <div className="flex flex-wrap items-end gap-2">
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.markov.fromEquipment")}
          <select
            className={RAMS_SELECT_CLASS}
            value=""
            onChange={(e) => {
              const eq = equipment.find((x) => x.equipment_id === Number(e.target.value));
              if (eq) fromEquipment(eq);
            }}
          >
            <option value="">{t("rams.fmeca.choose")}</option>
            {equipment.map((eq) => (
              <option
                key={eq.equipment_id}
                value={eq.equipment_id}
                disabled={eq.mtbf_hours == null || eq.mttr_hours == null}
              >
                {eq.mtbf_hours != null && eq.mttr_hours != null
                  ? t("rams.advanced.markov.equipmentOption", {
                      code: eq.asset_code,
                      mtbf: formatNumber(Math.round(eq.mtbf_hours)),
                      mttr: formatDecimal(eq.mttr_hours, 1),
                    })
                  : t("rams.system.equipmentNoFailure", { code: eq.asset_code })}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div className="mt-3 grid gap-4 lg:grid-cols-2">
        <div>
          <h4 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("rams.advanced.markov.states")}
          </h4>
          <div className={`${RAMS_TABLE_FRAME} mt-1`}>
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-surface-2 text-[11px] uppercase text-text-secondary">
                  <th className="px-2 py-2 text-start">{t("rams.advanced.markov.state")}</th>
                  <th className="px-2 py-2 text-start">{t("rams.advanced.markov.up")}</th>
                  <th className="px-2 py-2" />
                </tr>
              </thead>
              <tbody>
                {states.map((s) => (
                  <tr key={s.id} className="border-t border-surface-border">
                    <td className="px-2 py-1">
                      <Input
                        className="h-9"
                        aria-label={t("rams.advanced.markov.state")}
                        value={s.label}
                        onChange={(e) =>
                          setStates((xs) =>
                            xs.map((x) => (x.id === s.id ? { ...x, label: e.target.value } : x)),
                          )
                        }
                      />
                    </td>
                    <td className="px-2 py-1">
                      <input
                        type="checkbox"
                        className="h-4 w-4 accent-primary"
                        aria-label={t("rams.advanced.markov.upOf", { state: s.label })}
                        checked={s.up}
                        onChange={(e) =>
                          setStates((xs) =>
                            xs.map((x) => (x.id === s.id ? { ...x, up: e.target.checked } : x)),
                          )
                        }
                      />
                    </td>
                    <td className="px-2 py-1 text-end">
                      <Button
                        variant="ghost"
                        className="h-9 w-9 p-0"
                        aria-label={t("rams.advanced.remove")}
                        onClick={() => {
                          setStates((xs) => xs.filter((x) => x.id !== s.id));
                          setTransitions((xs) =>
                            xs.filter((x) => x.from !== s.id && x.to !== s.id),
                          );
                        }}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <Button
            variant="outline"
            className="mt-2 h-10"
            onClick={() =>
              setStates((xs) => [
                ...xs,
                {
                  id: newNodeId("m"),
                  label: t("rams.advanced.markov.newState", { n: xs.length + 1 }),
                  up: xs.length === 0,
                },
              ])
            }
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("rams.advanced.markov.addState")}
          </Button>
        </div>
        <div>
          <h4 className="text-xs font-semibold uppercase tracking-wide text-text-secondary">
            {t("rams.advanced.markov.transitions")}
          </h4>
          <div className={`${RAMS_TABLE_FRAME} mt-1`}>
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-surface-2 text-[11px] uppercase text-text-secondary">
                  <th className="px-2 py-2 text-start">{t("rams.advanced.markov.from")}</th>
                  <th className="px-2 py-2 text-start">{t("rams.advanced.markov.to")}</th>
                  <th className="px-2 py-2 text-start">{t("rams.advanced.markov.rate")}</th>
                  <th className="px-2 py-2" />
                </tr>
              </thead>
              <tbody>
                {transitions.map((x) => (
                  <tr key={x.key} className="border-t border-surface-border">
                    {(["from", "to"] as const).map((k) => (
                      <td key={k} className="px-2 py-1">
                        <select
                          className={cn(RAMS_SELECT_CLASS, "min-w-0")}
                          aria-label={t(`rams.advanced.markov.${k}`)}
                          value={x[k]}
                          onChange={(e) =>
                            setTransitions((xs) =>
                              xs.map((y) => (y.key === x.key ? { ...y, [k]: e.target.value } : y)),
                            )
                          }
                        >
                          {states.map((s) => (
                            <option key={s.id} value={s.id}>
                              {s.label}
                            </option>
                          ))}
                        </select>
                      </td>
                    ))}
                    <td className="px-2 py-1">
                      <Input
                        className="h-9 w-28"
                        inputMode="decimal"
                        aria-label={t("rams.advanced.markov.rate")}
                        value={x.rate}
                        onChange={(e) =>
                          setTransitions((xs) =>
                            xs.map((y) => (y.key === x.key ? { ...y, rate: e.target.value } : y)),
                          )
                        }
                      />
                    </td>
                    <td className="px-2 py-1 text-end">
                      <Button
                        variant="ghost"
                        className="h-9 w-9 p-0"
                        aria-label={t("rams.advanced.remove")}
                        onClick={() => setTransitions((xs) => xs.filter((y) => y.key !== x.key))}
                      >
                        <Trash2 className="h-4 w-4" aria-hidden />
                      </Button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <Button
            variant="outline"
            className="mt-2 h-10"
            disabled={states.length < 2}
            onClick={() =>
              setTransitions((xs) => [
                ...xs,
                {
                  key: newNodeId("t"),
                  from: states[0]?.id ?? "",
                  to: states[1]?.id ?? "",
                  rate: "",
                },
              ])
            }
          >
            <Plus className="h-4 w-4" aria-hidden />
            {t("rams.advanced.markov.addTransition")}
          </Button>
        </div>
      </div>
      <div className="mt-3 flex justify-end">
        <Button
          className="h-11"
          disabled={!valid || busy}
          onClick={async () => {
            setBusy(true);
            try {
              setResult(await evaluateReliabilityMarkov(input));
            } catch {
              setResult(null);
            } finally {
              setBusy(false);
            }
          }}
        >
          <Play className="h-4 w-4" aria-hidden />
          {t("rams.advanced.compute")}
        </Button>
      </div>
      {result ? (
        <div className="mt-3 grid gap-3">
          <div className="grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-3">
            <KpiTile
              label={t("rams.advanced.markov.availability")}
              value={formatDecimal(result.availability * 100, 3)}
              unit="%"
            />
            <KpiTile
              label={t("rams.advanced.markov.frequency")}
              value={formatDecimal(result.failures_per_year, 2)}
              unit={t("rams.advanced.perYear")}
            />
            <KpiTile
              label={t("rams.advanced.markov.mut")}
              value={
                result.mean_up_time_hours == null
                  ? "—"
                  : formatNumber(Math.round(result.mean_up_time_hours))
              }
              unit="h"
            />
            <KpiTile
              label={t("rams.advanced.markov.mdt")}
              value={
                result.mean_down_time_hours == null
                  ? "—"
                  : formatDecimal(result.mean_down_time_hours, 1)
              }
              unit="h"
            />
          </div>
          <div className={RAMS_TABLE_FRAME}>
            <table className="w-full text-sm">
              <thead>
                <tr className="bg-surface-2 text-[11px] uppercase text-text-secondary">
                  <th className="px-3 py-2 text-start">{t("rams.advanced.markov.state")}</th>
                  <th className="px-3 py-2 text-end">{t("rams.advanced.markov.probability")}</th>
                </tr>
              </thead>
              <tbody>
                {result.states.map((s) => (
                  <tr key={s.id} className="h-9 border-t border-surface-border">
                    <td className="px-3">
                      {s.label}{" "}
                      <span className="text-xs text-text-muted">
                        (
                        {s.up
                          ? t("rams.advanced.markov.upShort")
                          : t("rams.advanced.markov.downShort")}
                        )
                      </span>
                    </td>
                    <td className="px-3 text-end font-mono">
                      {formatDecimal(s.probability * 100, 4)} %
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      ) : null}
    </RamsCard>
  );
}

function SimulationTool({
  studyId,
  rbd,
  missionHours,
}: {
  studyId: number;
  rbd: RbdSpec | null;
  missionHours: number | null;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal, formatNumber, formatCurrency } = useFormatters();
  const [trials, setTrials] = useState("10000");
  const [seed, setSeed] = useState("");
  const [costH, setCostH] = useState("");
  const [costR, setCostR] = useState("");
  const [result, setResult] = useState<SimulationResult | null>(null);
  const [busy, setBusy] = useState(false);
  if (!rbd || missionHours == null || !(missionHours > 0)) {
    return (
      <RamsCard
        title={t("rams.advanced.simulation.title")}
        subtitle={t("rams.advanced.simulation.hint")}
      >
        <p className="text-sm text-text-muted">{t("rams.advanced.simulation.needRbd")}</p>
      </RamsCard>
    );
  }
  const trialsN = Math.round(num(trials));
  const spread = (
    s: { mean: number; p05: number; p50: number; p95: number },
    f: (v: number) => string,
  ) =>
    t("rams.advanced.simulation.spread", {
      mean: f(s.mean),
      p05: f(s.p05),
      p50: f(s.p50),
      p95: f(s.p95),
    });
  return (
    <RamsCard
      title={t("rams.advanced.simulation.title")}
      subtitle={t("rams.advanced.simulation.hint")}
    >
      <div className="grid gap-3 sm:grid-cols-4">
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.simulation.trials")}
          <Input
            className="h-10"
            inputMode="numeric"
            value={trials}
            onChange={(e) => setTrials(e.target.value)}
          />
        </label>
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.simulation.seed")}
          <Input
            className="h-10"
            inputMode="numeric"
            value={seed}
            onChange={(e) => setSeed(e.target.value)}
          />
        </label>
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.simulation.downtimeCost")}
          <Input
            className="h-10"
            inputMode="decimal"
            value={costH}
            onChange={(e) => setCostH(e.target.value)}
          />
        </label>
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.simulation.repairCost")}
          <Input
            className="h-10"
            inputMode="decimal"
            value={costR}
            onChange={(e) => setCostR(e.target.value)}
          />
        </label>
      </div>
      <p className="mt-2 text-xs text-text-muted">
        {t("rams.advanced.simulation.method", { hours: formatNumber(missionHours) })}
      </p>
      <div className="mt-3 flex justify-end">
        <Button
          className="h-11"
          disabled={busy || !(trialsN >= 1)}
          onClick={async () => {
            setBusy(true);
            try {
              setResult(
                await simulateReliabilityStudySystem({
                  study_id: studyId,
                  mission_hours: missionHours,
                  trials: trialsN,
                  seed: seed.trim() ? Math.round(num(seed)) : null,
                  spec: rbd,
                  downtime_cost_per_hour: Number.isFinite(num(costH)) ? num(costH) : null,
                  repair_cost: Number.isFinite(num(costR)) ? num(costR) : null,
                }),
              );
            } catch {
              setResult(null);
            } finally {
              setBusy(false);
            }
          }}
        >
          <Play className="h-4 w-4" aria-hidden />
          {busy ? t("rams.advanced.running") : t("rams.advanced.simulation.run")}
        </Button>
      </div>
      {result ? (
        result.issues.length > 0 ? (
          <div className="mt-3">
            <RamsCallout tone="warning" title={t("rams.system.incomplete")}>
              <ul className="list-disc ps-4">
                {result.issues.map((i, k) => (
                  <li key={k}>
                    {t(`rams.system.issue.${i.code}`, {
                      node: rbd.nodes.find((n) => n.id === i.node_id)?.label ?? "",
                    })}
                  </li>
                ))}
              </ul>
            </RamsCallout>
          </div>
        ) : (
          <div className="mt-3 grid gap-3">
            <div className="grid grid-cols-[repeat(auto-fit,minmax(180px,1fr))] gap-3">
              <KpiTile
                label={t("rams.advanced.simulation.availability")}
                value={formatDecimal(result.availability.mean * 100, 2)}
                unit="%"
                hint={spread(result.availability, (v) => `${formatDecimal(v * 100, 2)} %`)}
              />
              <KpiTile
                label={t("rams.advanced.simulation.reliability")}
                value={formatDecimal(result.mission_reliability * 100, 1)}
                unit="%"
                hint={t("rams.advanced.simulation.reliabilityHint")}
              />
              <KpiTile
                label={t("rams.advanced.simulation.failures")}
                value={formatDecimal(result.system_failures.mean, 2)}
                hint={spread(result.system_failures, (v) => formatDecimal(v, 0))}
              />
              <KpiTile
                label={t("rams.advanced.simulation.downtime")}
                value={formatDecimal(result.downtime_hours.mean, 1)}
                unit="h"
                hint={spread(result.downtime_hours, (v) => formatDecimal(v, 0))}
              />
              {result.cost ? (
                <KpiTile
                  label={t("rams.advanced.simulation.cost")}
                  value={formatCurrency(result.cost.mean)}
                  hint={spread(result.cost, (v) => formatCurrency(v))}
                />
              ) : null}
            </div>
            <div className={RAMS_TABLE_FRAME}>
              <table className="w-full text-sm">
                <thead>
                  <tr className="bg-surface-2 text-[11px] uppercase text-text-secondary">
                    <th className="px-3 py-2 text-start">{t("rams.system.colBlock")}</th>
                    <th className="px-3 py-2 text-end">β</th>
                    <th className="px-3 py-2 text-end">η (h)</th>
                    <th className="px-3 py-2 text-end">{t("rams.system.mttr")}</th>
                    <th className="px-3 py-2 text-end">
                      {t("rams.advanced.simulation.meanFailures")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {result.blocks.map((b) => (
                    <tr key={b.id} className="h-9 border-t border-surface-border">
                      <td className="px-3">{b.label}</td>
                      <td className="px-3 text-end font-mono">{formatDecimal(b.beta, 2)}</td>
                      <td className="px-3 text-end font-mono">{formatNumber(Math.round(b.eta))}</td>
                      <td
                        className="px-3 text-end font-mono"
                        title={t(`rams.advanced.simulation.mttrSource.${b.mttr_source}`)}
                      >
                        {formatDecimal(b.mttr_hours, 1)}
                      </td>
                      <td className="px-3 text-end font-mono">
                        {formatDecimal(b.mean_failures, 2)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <p className="text-xs text-text-muted">
              {t("rams.advanced.simulation.footer", {
                trials: formatNumber(result.trials),
                seed: result.seed,
              })}
            </p>
          </div>
        )
      ) : null}
    </RamsCard>
  );
}

function EventTreeTool() {
  const { t } = useTranslation("reliability");
  const { formatDecimal } = useFormatters();
  const [initiator, setInitiator] = useState("");
  const [frequency, setFrequency] = useState("");
  const [barriers, setBarriers] = useState<{ key: string; label: string; p: string }[]>([]);
  const [consequences, setConsequences] = useState<Record<number, string>>({});
  const [result, setResult] = useState<EventTreeResult | null>(null);
  const [busy, setBusy] = useState(false);
  const freqN = num(frequency);
  const valid =
    initiator.trim() !== "" &&
    freqN > 0 &&
    barriers.length >= 1 &&
    barriers.length <= 6 &&
    barriers.every((b) => b.label.trim() !== "" && num(b.p) >= 0 && num(b.p) <= 1);
  const fmt = (v: number) => (v > 0 && v < 0.001 ? v.toExponential(2) : formatDecimal(v, 4));
  return (
    <RamsCard
      title={t("rams.advanced.event_tree.title")}
      subtitle={t("rams.advanced.event_tree.hint")}
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.event_tree.initiator")}
          <Input
            className="h-10"
            value={initiator}
            onChange={(e) => setInitiator(e.target.value)}
          />
        </label>
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.advanced.event_tree.frequency")}
          <Input
            className="h-10"
            inputMode="decimal"
            value={frequency}
            onChange={(e) => setFrequency(e.target.value)}
          />
        </label>
      </div>
      <h4 className="mt-3 text-xs font-semibold uppercase tracking-wide text-text-secondary">
        {t("rams.advanced.event_tree.barriers")}
      </h4>
      <div className="mt-1 grid gap-2">
        {barriers.map((b, i) => (
          <div
            key={b.key}
            className="grid grid-cols-[1.5rem_minmax(0,1fr)_8rem_2.5rem] items-center gap-2"
          >
            <span className="font-mono text-xs text-text-muted">{i + 1}</span>
            <Input
              className="h-10"
              aria-label={t("rams.advanced.event_tree.barrier")}
              value={b.label}
              onChange={(e) =>
                setBarriers((xs) =>
                  xs.map((x) => (x.key === b.key ? { ...x, label: e.target.value } : x)),
                )
              }
            />
            <Input
              className="h-10"
              inputMode="decimal"
              aria-label={t("rams.advanced.event_tree.failureProbability")}
              placeholder={t("rams.advanced.event_tree.failureProbability")}
              value={b.p}
              onChange={(e) =>
                setBarriers((xs) =>
                  xs.map((x) => (x.key === b.key ? { ...x, p: e.target.value } : x)),
                )
              }
            />
            <Button
              variant="ghost"
              className="h-10 w-10 p-0"
              aria-label={t("rams.advanced.remove")}
              onClick={() => setBarriers((xs) => xs.filter((x) => x.key !== b.key))}
            >
              <Trash2 className="h-4 w-4" aria-hidden />
            </Button>
          </div>
        ))}
      </div>
      <div className="mt-2 flex flex-wrap justify-between gap-2">
        <Button
          variant="outline"
          className="h-10"
          disabled={barriers.length >= 6}
          onClick={() => setBarriers((xs) => [...xs, { key: newNodeId("b"), label: "", p: "" }])}
        >
          <Plus className="h-4 w-4" aria-hidden />
          {t("rams.advanced.event_tree.addBarrier")}
        </Button>
        <Button
          className="h-11"
          disabled={!valid || busy}
          onClick={async () => {
            setBusy(true);
            try {
              setResult(
                await evaluateReliabilityEventTree({
                  initiating_event: initiator.trim(),
                  frequency_per_year: freqN,
                  barriers: barriers.map((b) => ({
                    label: b.label.trim(),
                    failure_probability: num(b.p),
                  })),
                  consequences: Array.from(
                    { length: 2 ** barriers.length },
                    (_, i) => consequences[i]?.trim() || null,
                  ),
                }),
              );
            } catch {
              setResult(null);
            } finally {
              setBusy(false);
            }
          }}
        >
          <Play className="h-4 w-4" aria-hidden />
          {t("rams.advanced.compute")}
        </Button>
      </div>
      {result ? (
        <div className={`${RAMS_TABLE_FRAME} mt-3`}>
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-surface-2 text-[11px] uppercase text-text-secondary">
                <th className="px-3 py-2 text-start">{t("rams.advanced.event_tree.sequence")}</th>
                <th className="px-3 py-2 text-start">
                  {t("rams.advanced.event_tree.consequence")}
                </th>
                <th className="px-3 py-2 text-end">{t("rams.advanced.event_tree.probability")}</th>
                <th className="px-3 py-2 text-end">{t("rams.advanced.event_tree.perYear")}</th>
              </tr>
            </thead>
            <tbody>
              {result.sequences.map((s) => (
                <tr key={s.index} className="border-t border-surface-border">
                  <td className="px-3 py-1.5 text-xs">
                    {s.failed
                      .map((f, i) =>
                        t(f ? "rams.advanced.event_tree.fails" : "rams.advanced.event_tree.works", {
                          barrier: barriers[i]?.label ?? "",
                        }),
                      )
                      .join(" · ")}
                  </td>
                  <td className="px-3 py-1.5">
                    <Input
                      className="h-9"
                      aria-label={t("rams.advanced.event_tree.consequence")}
                      value={consequences[s.index] ?? s.consequence ?? ""}
                      onChange={(e) =>
                        setConsequences((c) => ({ ...c, [s.index]: e.target.value }))
                      }
                    />
                  </td>
                  <td className="px-3 py-1.5 text-end font-mono">{fmt(s.probability)}</td>
                  <td className="px-3 py-1.5 text-end font-mono">{fmt(s.frequency_per_year)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </RamsCard>
  );
}
