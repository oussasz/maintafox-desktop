/**
 * RCM / FMECA → PM loop dialogs (PRD §6.10.4–6.10.5): the RCM decision logic with its path,
 * the actions that create a PM plan revision or a work order (linked both ways), and the
 * MIL-STD-1629A criticality number of an FMECA item.
 */
import { Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Button,
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { woTypeLabel } from "@/components/wo/use-wo-type-labels";
import { listPmPlans } from "@/services/pm-service";
import {
  applyRcmDecisionLogic,
  computeFmecaCriticality,
  createPmRevisionFromRamsSource,
  createWoFromRamsSource,
  getFmecaCriticalityEvidence,
  listRcmDecisionLogic,
  previewRcmRecommendation,
} from "@/services/reliability-service";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { toErrorMessage } from "@/utils/errors";
import type {
  FmecaCriticalityEvidence,
  RamsActionSourceType,
  RcmAnswers,
  RcmDecisionLogic,
  RcmRecommendation,
} from "@shared/ipc-types";

const TACTICS = [
  "condition_based",
  "time_based",
  "failure_finding",
  "run_to_failure",
  "redesign",
] as const;

const DEFAULT_ANSWERS: RcmAnswers = {
  failure_evident: true,
  safety_or_environmental: false,
  operational_impact: true,
  multiple_failure_affects_safety: false,
  on_condition_feasible: false,
  scheduled_restoration_feasible: false,
  scheduled_discard_feasible: false,
  failure_finding_feasible: false,
  combination_feasible: false,
  weibull_beta: null,
};

type BoolKey = Exclude<keyof RcmAnswers, "weibull_beta">;

/** Questions shown for the current answers, in the order of the decision diagram. */
export function visibleQuestions(a: RcmAnswers): BoolKey[] {
  const q: BoolKey[] = ["failure_evident"];
  if (a.failure_evident) {
    q.push("safety_or_environmental");
    if (!a.safety_or_environmental) q.push("operational_impact");
  } else {
    q.push("multiple_failure_affects_safety");
  }
  q.push("on_condition_feasible", "scheduled_restoration_feasible", "scheduled_discard_feasible");
  if (!a.failure_evident) q.push("failure_finding_feasible");
  if (a.failure_evident && a.safety_or_environmental) q.push("combination_feasible");
  return q;
}

function YesNo({
  value,
  onChange,
  labelYes,
  labelNo,
}: {
  value: boolean;
  onChange: (v: boolean) => void;
  labelYes: string;
  labelNo: string;
}) {
  return (
    <div className="flex gap-2" role="radiogroup">
      {[true, false].map((v) => (
        <button
          key={String(v)}
          type="button"
          role="radio"
          aria-checked={value === v}
          onClick={() => onChange(v)}
          className={`min-h-10 min-w-16 rounded-md border px-3 text-sm ${
            value === v ? "border-primary bg-primary/10 font-semibold" : "border-surface-border"
          }`}
        >
          {v ? labelYes : labelNo}
        </button>
      ))}
    </div>
  );
}

/** Creates a PM plan revision or a work order from an FMECA item / RCM decision. */
function SourceActions({
  sourceType,
  sourceId,
  allowPm,
  onDone,
}: {
  sourceType: RamsActionSourceType;
  sourceId: number;
  allowPm: boolean;
  onDone: (message: string) => void;
}) {
  const { t, i18n } = useTranslation("reliability");
  const [plans, setPlans] = useState<{ id: number; label: string }[]>([]);
  const [planId, setPlanId] = useState("");
  const [note, setNote] = useState("");
  const woTypes = useWorkOrderTypesCatalog((s) => s.types);
  const loadWoTypes = useWorkOrderTypesCatalog((s) => s.load);
  const [woType, setWoType] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadWoTypes();
    listPmPlans({})
      .then((ps) => setPlans(ps.map((p) => ({ id: p.id, label: `${p.code} — ${p.title}` }))))
      .catch((err: unknown) => setError(toErrorMessage(err)));
  }, [loadWoTypes]);
  const active = useMemo(() => woTypes.filter((w) => w.is_active), [woTypes]);

  const run = async (fn: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    try {
      onDone(await fn());
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 border-t border-surface-border pt-3">
      {allowPm ? (
        <div className="space-y-2">
          <p className="text-sm font-medium">{t("rcmLoop.pmRevision")}</p>
          <Select value={planId} onValueChange={setPlanId}>
            <SelectTrigger aria-label={t("rcmLoop.pmPlan")}>
              <SelectValue placeholder={t("rcmLoop.pmPlanPlaceholder")} />
            </SelectTrigger>
            <SelectContent>
              {plans.map((p) => (
                <SelectItem key={p.id} value={String(p.id)}>
                  {p.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Textarea
            aria-label={t("rcmLoop.changeNote")}
            placeholder={t("rcmLoop.changeNote")}
            rows={2}
            value={note}
            onChange={(e) => setNote(e.target.value)}
          />
          <Button
            size="sm"
            disabled={busy || !planId || !note.trim()}
            onClick={() =>
              void run(async () => {
                const r = await createPmRevisionFromRamsSource({
                  source_type: sourceType,
                  source_id: sourceId,
                  pm_plan_id: Number(planId),
                  change_note: note.trim(),
                });
                const plan = plans.find((p) => String(p.id) === planId)?.label ?? "";
                return t("rcmLoop.revisionCreated", { no: r.pm_version_no, plan });
              })
            }
          >
            {t("rcmLoop.createRevision")}
          </Button>
        </div>
      ) : null}
      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-48 space-y-1">
          <Label>{t("rcmLoop.woType")}</Label>
          <Select value={woType} onValueChange={setWoType}>
            <SelectTrigger aria-label={t("rcmLoop.woType")}>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {active.map((w) => (
                <SelectItem key={w.code} value={w.code}>
                  {woTypeLabel(w, i18n.language)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={busy || !woType}
          onClick={() =>
            void run(async () => {
              const r = await createWoFromRamsSource({
                source_type: sourceType,
                source_id: sourceId,
                type_code: woType,
              });
              return t("rcmLoop.woCreated", { code: r.work_order_code ?? "" });
            })
          }
        >
          {t("rcmLoop.createWo")}
        </Button>
      </div>
      {busy ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
      {error ? (
        <p role="alert" className="text-sm text-status-danger">
          {error}
        </p>
      ) : null}
    </div>
  );
}

export function RcmDecisionDialog({
  studyId,
  decisionId,
  onClose,
  onSaved,
}: {
  studyId: number;
  decisionId: number;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("reliability");
  const [logic, setLogic] = useState<RcmDecisionLogic | null>(null);
  const [answers, setAnswers] = useState<RcmAnswers>(DEFAULT_ANSWERS);
  const [betaRaw, setBetaRaw] = useState("");
  const [preview, setPreview] = useState<RcmRecommendation | null>(null);
  const [tactic, setTactic] = useState("");
  const [justification, setJustification] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    const all = await listRcmDecisionLogic(studyId);
    const d = all.find((x) => x.decision_id === decisionId) ?? null;
    setLogic(d);
    if (d?.answers) {
      setAnswers(d.answers);
      setBetaRaw(d.answers.weibull_beta != null ? String(d.answers.weibull_beta) : "");
      setTactic(d.selected_tactic);
      setJustification(d.justification);
    }
  };
  useEffect(() => {
    reload().catch((err: unknown) => setError(toErrorMessage(err)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [studyId, decisionId]);

  const beta = betaRaw.trim() === "" ? null : Number(betaRaw.replace(",", "."));
  const betaInvalid = beta != null && !Number.isFinite(beta);
  useEffect(() => {
    if (betaInvalid) return;
    previewRcmRecommendation({ ...answers, weibull_beta: beta })
      .then(setPreview)
      .catch((err: unknown) => setError(toErrorMessage(err)));
  }, [answers, beta, betaInvalid]);

  const retained = tactic || preview?.recommended_tactic || "";
  const needsJustification = preview != null && retained !== preview.recommended_tactic;

  const apply = async () => {
    if (!logic) return;
    setError(null);
    try {
      const out = await applyRcmDecisionLogic({
        decision_id: decisionId,
        expected_row_version: logic.row_version,
        answers: { ...answers, weibull_beta: beta },
        selected_tactic: retained || null,
        justification: justification.trim() || null,
      });
      setLogic(out);
      onSaved();
    } catch (err) {
      setError(toErrorMessage(err));
    }
  };

  return (
    <Dialog open onOpenChange={(o) => (!o ? onClose() : undefined)}>
      <DialogContent className="max-h-[90vh] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("rcmLoop.logicTitle")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          {visibleQuestions(answers).map((key) => (
            <div key={key} className="space-y-1">
              <p className="text-sm">{t(`rcmLoop.questions.${key}`)}</p>
              <YesNo
                value={answers[key]}
                onChange={(v) => setAnswers((a) => ({ ...a, [key]: v }))}
                labelYes={t("rcmLoop.yes")}
                labelNo={t("rcmLoop.no")}
              />
            </div>
          ))}
          <div className="max-w-xs space-y-1">
            <Label htmlFor="rcm-beta">{t("rcmLoop.questions.weibull_beta")}</Label>
            <Input
              id="rcm-beta"
              inputMode="decimal"
              value={betaRaw}
              onChange={(e) => setBetaRaw(e.target.value)}
            />
            {betaInvalid ? (
              <p className="text-xs text-status-danger">{t("rcmLoop.invalidNumber")}</p>
            ) : null}
          </div>
          {preview ? (
            <div
              className="rounded-md border border-surface-border p-3 text-sm"
              data-testid="rcm-preview"
            >
              <p>
                {t("rcmLoop.consequence")} :{" "}
                <strong>{t(`rcmLoop.consequences.${preview.consequence_category}`)}</strong>
              </p>
              <p>
                {t("rcmLoop.recommended")} :{" "}
                <strong>{t(`rcmLoop.tactics.${preview.recommended_tactic}`)}</strong>
              </p>
              {preview.redesign_compulsory ? (
                <p className="text-status-danger">{t("rcmLoop.compulsory")}</p>
              ) : null}
              <p className="mt-1 text-xs text-text-muted">
                {t("rcmLoop.path")} : {preview.path.join(" → ")}
              </p>
            </div>
          ) : null}
          <div className="space-y-1">
            <Label>{t("rcmLoop.retained")}</Label>
            <Select value={retained} onValueChange={setTactic}>
              <SelectTrigger aria-label={t("rcmLoop.retained")}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TACTICS.map((x) => (
                  <SelectItem key={x} value={x}>
                    {t(`rcmLoop.tactics.${x}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Textarea
            aria-label={t("rcmLoop.justification")}
            placeholder={t("rcmLoop.justification")}
            rows={2}
            value={justification}
            onChange={(e) => setJustification(e.target.value)}
          />
          <Button
            disabled={!logic || betaInvalid || (needsJustification && !justification.trim())}
            onClick={() => void apply()}
          >
            {t("rcmLoop.apply")}
          </Button>
          {logic?.pm_plan_code ? (
            <p className="text-sm">
              {t("rcmLoop.linkedPm", { plan: logic.pm_plan_code, no: logic.pm_version_no ?? "" })}
            </p>
          ) : null}
          {logic?.work_order_code ? (
            <p className="text-sm">{t("rcmLoop.linkedWo", { code: logic.work_order_code })}</p>
          ) : null}
          {logic?.recommended_tactic ? (
            <SourceActions
              sourceType="rcm_decision"
              sourceId={decisionId}
              allowPm={["condition_based", "time_based", "failure_finding"].includes(
                logic.selected_tactic,
              )}
              onDone={(m) => {
                setMessage(m);
                void reload();
                onSaved();
              }}
            />
          ) : null}
          {message ? <p className="text-sm text-status-success">{message}</p> : null}
          {error ? (
            <p role="alert" className="text-sm text-status-danger">
              {error}
            </p>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}

const BETA_OPTIONS = ["1", "0.5", "0.1", "0"] as const;

export function FmecaActionsDialog({
  itemId,
  rowVersion,
  onClose,
  onSaved,
}: {
  itemId: number;
  rowVersion: number;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation("reliability");
  const [evidence, setEvidence] = useState<FmecaCriticalityEvidence | null>(null);
  const [beta, setBeta] = useState<string>("1");
  const [alpha, setAlpha] = useState("");
  const [lambda, setLambda] = useState("");
  const [hours, setHours] = useState("");
  const [rv, setRv] = useState(rowVersion);
  const [result, setResult] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getFmecaCriticalityEvidence(itemId)
      .then((e) => {
        setEvidence(e);
        if (e.mode_ratio != null) setAlpha(String(e.mode_ratio));
        if (e.failure_rate_per_hour != null) setLambda(String(e.failure_rate_per_hour));
      })
      .catch((err: unknown) => setError(toErrorMessage(err)));
  }, [itemId]);

  const num = (s: string) => (s.trim() === "" ? null : Number(s.replace(",", ".")));
  const compute = async () => {
    setError(null);
    const a = num(alpha);
    const l = num(lambda);
    const h = num(hours);
    if (
      h == null ||
      !Number.isFinite(h) ||
      (a != null && !Number.isFinite(a)) ||
      (l != null && !Number.isFinite(l))
    ) {
      setError(t("rcmLoop.invalidNumber"));
      return;
    }
    try {
      const c = await computeFmecaCriticality({
        item_id: itemId,
        expected_row_version: rv,
        effect_probability: Number(beta),
        mode_ratio: a,
        failure_rate_per_hour: l,
        operating_hours: h,
      });
      setRv(c.row_version);
      setResult(
        t("rcmLoop.cm", {
          cm: c.criticality_number.toPrecision(3),
          cr: c.analysis_criticality.toPrecision(3),
        }),
      );
      onSaved();
    } catch (err) {
      setError(toErrorMessage(err));
    }
  };

  return (
    <Dialog open onOpenChange={(o) => (!o ? onClose() : undefined)}>
      <DialogContent className="max-h-[90vh] max-w-xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t("rcmLoop.openActions")}</DialogTitle>
        </DialogHeader>
        <div className="space-y-3">
          <p className="text-sm font-medium">{t("rcmLoop.criticality")}</p>
          <p className="text-xs text-text-muted">
            {evidence && evidence.mode_ratio != null && evidence.failure_rate_per_hour != null
              ? t("rcmLoop.evidence", {
                  mode: evidence.mode_failures,
                  total: evidence.total_failures,
                  date: evidence.kpi_period_end ?? "—",
                })
              : t("rcmLoop.noEvidence")}
          </p>
          <div className="grid grid-cols-2 gap-2">
            <div className="space-y-1">
              <Label>{t("rcmLoop.beta")}</Label>
              <Select value={beta} onValueChange={setBeta}>
                <SelectTrigger aria-label={t("rcmLoop.beta")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {BETA_OPTIONS.map((b) => (
                    <SelectItem key={b} value={b}>
                      {t(`rcmLoop.betaOptions.${b}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="fmeca-alpha">{t("rcmLoop.alpha")}</Label>
              <Input
                id="fmeca-alpha"
                inputMode="decimal"
                value={alpha}
                onChange={(e) => setAlpha(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="fmeca-lambda">{t("rcmLoop.lambda")}</Label>
              <Input
                id="fmeca-lambda"
                inputMode="decimal"
                value={lambda}
                onChange={(e) => setLambda(e.target.value)}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="fmeca-hours">{t("rcmLoop.hours")}</Label>
              <Input
                id="fmeca-hours"
                inputMode="decimal"
                value={hours}
                onChange={(e) => setHours(e.target.value)}
              />
            </div>
          </div>
          <Button onClick={() => void compute()}>{t("rcmLoop.compute")}</Button>
          {result ? (
            <p className="text-sm font-medium" data-testid="fmeca-cm">
              {result}
            </p>
          ) : null}
          <SourceActions
            sourceType="fmeca_item"
            sourceId={itemId}
            allowPm
            onDone={(m) => {
              setMessage(m);
              onSaved();
            }}
          />
          {message ? <p className="text-sm text-status-success">{message}</p> : null}
          {error ? (
            <p role="alert" className="text-sm text-status-danger">
              {error}
            </p>
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  );
}
