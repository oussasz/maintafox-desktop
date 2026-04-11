/**
 * WoCloseOutPanel.tsx
 *
 * Four-section close-out panel for a Work Order:
 *   S1 — Symptom & Narrative
 *   S2 — Failure Analysis (mode / cause / effect)
 *   S3 — Action Performed (corrective action, root cause, repair type, service cost)
 *   S4 — Return to Service (mechanical completion → verification → closure)
 *
 * Phase 2 – Sub-phase 05 – File 03 – Sprint S3.
 */

import {
  AlertCircle,
  CheckCircle2,
  Loader2,
  Save,
  WrenchIcon,
  ShieldCheck,
  Lock,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { WoCostSummaryCard } from "@/components/wo/WoCostSummaryCard";
import { useSession } from "@/hooks/use-session";
import { useStepUp } from "@/hooks/use-step-up";
import { getLookupValues } from "@/services/lookup-service";
import {
  saveFailureDetail,
  saveVerification,
  closeWo,
  updateWoRca,
  updateServiceCost,
  CloseoutBlockingError,
} from "@/services/wo-closeout-service";
import { completeMechanically } from "@/services/wo-execution-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrder } from "@shared/ipc-types";
import type { LookupValueOption } from "@shared/ipc-types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoCloseOutPanelProps {
  wo: WorkOrder;
  canEdit: boolean;
  onClosed: () => void;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

type RepairType = "temporary" | "permanent" | "na";

const MECH_COMPLETABLE_STATUSES = new Set(["in_progress", "paused"]);
const VERIFIABLE_STATUSES = new Set(["mechanically_complete"]);
const CLOSEABLE_STATUSES = new Set(["technically_verified"]);

function SectionHeader({ icon, title }: { icon: React.ReactNode; title: string }) {
  return (
    <div className="flex items-center gap-2 border-b border-surface-border pb-2">
      <span className="text-primary">{icon}</span>
      <h3 className="text-sm font-semibold text-text-primary">{title}</h3>
    </div>
  );
}

function ErrorBanner({ message }: { message: string }) {
  return (
    <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
      <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}

function SuccessBanner({ message }: { message: string }) {
  return (
    <div className="flex items-center gap-2 rounded-md border border-green-500/50 bg-green-50 p-3 text-sm text-green-700">
      <CheckCircle2 className="h-4 w-4 shrink-0" />
      <span>{message}</span>
    </div>
  );
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoCloseOutPanel({ wo, canEdit, onClosed }: WoCloseOutPanelProps) {
  const { info } = useSession();
  const { withStepUp, StepUpDialogElement } = useStepUp();

  // Track current WO state (gets updated after transitions)
  const [currentWo, setCurrentWo] = useState<WorkOrder>(wo);
  const statusCode = currentWo.status_code ?? "draft";

  // ── Lookups ────────────────────────────────────────────────────────────
  const [symptoms, setSymptoms] = useState<LookupValueOption[]>([]);
  const [failureModes, setFailureModes] = useState<LookupValueOption[]>([]);
  const [failureCauses, setFailureCauses] = useState<LookupValueOption[]>([]);
  const [failureEffects, setFailureEffects] = useState<LookupValueOption[]>([]);

  useEffect(() => {
    void Promise.all([
      getLookupValues("WORK.SYMPTOMS").catch(() => [] as LookupValueOption[]),
      getLookupValues("WORK.FAILURE_MODES").catch(() => [] as LookupValueOption[]),
      getLookupValues("WORK.FAILURE_CAUSES").catch(() => [] as LookupValueOption[]),
      getLookupValues("WORK.FAILURE_EFFECTS").catch(() => [] as LookupValueOption[]),
    ]).then(([s, m, c, e]) => {
      setSymptoms(s);
      setFailureModes(m);
      setFailureCauses(c);
      setFailureEffects(e);
    });
  }, []);

  // ── S1 + S2 + S3 form state ────────────────────────────────────────────
  const [symptomId, setSymptomId] = useState<number | null>(null);
  const [narrative, setNarrative] = useState("");
  const [causeNotDetermined, setCauseNotDetermined] = useState(false);
  const [failureModeId, setFailureModeId] = useState<number | null>(null);
  const [failureCauseId, setFailureCauseId] = useState<number | null>(null);
  const [failureEffectId, setFailureEffectId] = useState<number | null>(null);
  const [repairType, setRepairType] = useState<RepairType>("na");
  const [correctiveAction, setCorrectiveAction] = useState("");
  const [rootCauseSummary, setRootCauseSummary] = useState("");
  const [serviceCost, setServiceCost] = useState("");

  const [savingDetails, setSavingDetails] = useState(false);
  const [detailError, setDetailError] = useState<string | null>(null);
  const [detailSuccess, setDetailSuccess] = useState(false);

  // ── S4 — Mechanical completion ────────────────────────────────────────
  const [completingMech, setCompletingMech] = useState(false);
  const [mechError, setMechError] = useState<string | null>(null);

  // ── S4 — Verification form ────────────────────────────────────────────
  const [verificationResult, setVerificationResult] = useState<"pass" | "fail" | "monitor" | "">(
    "",
  );
  const [returnToServiceConfirmed, setReturnToServiceConfirmed] = useState(false);
  const [verificationNotes, setVerificationNotes] = useState("");
  const [savingVerification, setSavingVerification] = useState(false);
  const [verificationError, setVerificationError] = useState<string | null>(null);

  // ── S4 — Close ────────────────────────────────────────────────────────
  const [stepUpPin, setStepUpPin] = useState("");
  const [closing, setClosing] = useState(false);
  const [blockingErrors, setBlockingErrors] = useState<string[]>([]);
  const [closeError, setCloseError] = useState<string | null>(null);

  // Keep currentWo in sync with prop changes (e.g., parent refreshes)
  useEffect(() => {
    setCurrentWo(wo);
  }, [wo]);

  // ── Save S1-S3 details ─────────────────────────────────────────────────

  const handleSaveDetails = useCallback(async () => {
    if (!canEdit) return;
    setSavingDetails(true);
    setDetailError(null);
    setDetailSuccess(false);
    try {
      await saveFailureDetail({
        wo_id: currentWo.id,
        symptom_id: symptomId,
        failure_mode_id: causeNotDetermined ? null : failureModeId,
        failure_cause_id: causeNotDetermined ? null : failureCauseId,
        failure_effect_id: causeNotDetermined ? null : failureEffectId,
        is_temporary_repair: repairType === "temporary",
        is_permanent_repair: repairType === "permanent",
        cause_not_determined: causeNotDetermined,
        notes: narrative.trim() || null,
      });
      await updateWoRca({
        wo_id: currentWo.id,
        root_cause_summary: rootCauseSummary.trim() || null,
        corrective_action_summary: correctiveAction.trim() || null,
      });
      const costNum = parseFloat(serviceCost);
      if (!isNaN(costNum) && costNum >= 0) {
        await updateServiceCost(currentWo.id, costNum);
      }
      setDetailSuccess(true);
    } catch (err) {
      setDetailError(toErrorMessage(err));
    } finally {
      setSavingDetails(false);
    }
  }, [
    canEdit,
    currentWo,
    symptomId,
    causeNotDetermined,
    failureModeId,
    failureCauseId,
    failureEffectId,
    repairType,
    narrative,
    rootCauseSummary,
    correctiveAction,
    serviceCost,
  ]);

  // ── Mark mechanically complete ─────────────────────────────────────────

  const handleMechComplete = useCallback(() => {
    const actorId = info?.user_id;
    if (!canEdit || !actorId) return;
    setMechError(null);
    setCompletingMech(true);
    withStepUp(() =>
      completeMechanically({
        wo_id: currentWo.id,
        actor_id: actorId,
        expected_row_version: currentWo.row_version,
      }),
    )
      .then((updated) => setCurrentWo(updated))
      .catch((err: unknown) => setMechError(toErrorMessage(err)))
      .finally(() => setCompletingMech(false));
  }, [canEdit, currentWo, info, withStepUp]);

  // ── Submit verification ────────────────────────────────────────────────

  const handleSaveVerification = useCallback(() => {
    const actorId = info?.user_id;
    if (!canEdit || !actorId || !verificationResult) return;
    setSavingVerification(true);
    setVerificationError(null);
    withStepUp(() =>
      saveVerification({
        wo_id: currentWo.id,
        verified_by_id: actorId,
        result: verificationResult,
        return_to_service_confirmed: returnToServiceConfirmed,
        recurrence_risk_level: null,
        notes: verificationNotes.trim() || null,
        expected_row_version: currentWo.row_version,
      }),
    )
      .then(([, updatedWo]) => {
        setCurrentWo(updatedWo);
        setVerificationResult("");
        setReturnToServiceConfirmed(false);
        setVerificationNotes("");
      })
      .catch((err: unknown) => setVerificationError(toErrorMessage(err)))
      .finally(() => setSavingVerification(false));
  }, [
    canEdit,
    currentWo,
    info,
    withStepUp,
    verificationResult,
    returnToServiceConfirmed,
    verificationNotes,
  ]);

  // ── Close WO ───────────────────────────────────────────────────────────

  const handleClose = useCallback(() => {
    const actorId = info?.user_id;
    if (!canEdit || !actorId) return;
    setClosing(true);
    setBlockingErrors([]);
    setCloseError(null);
    withStepUp(() =>
      closeWo({
        wo_id: currentWo.id,
        actor_id: actorId,
        expected_row_version: currentWo.row_version,
      }),
    )
      .then(() => onClosed())
      .catch((err: unknown) => {
        if (err instanceof CloseoutBlockingError) {
          setBlockingErrors(err.blockingErrors);
        } else {
          setCloseError(toErrorMessage(err));
        }
      })
      .finally(() => setClosing(false));
  }, [canEdit, currentWo, info, withStepUp, onClosed]);

  // ── Render ──────────────────────────────────────────────────────────────

  const isDetailEditable = canEdit && !["closed", "cancelled"].includes(statusCode);

  return (
    <div className="space-y-6">
      {StepUpDialogElement}

      {/* ══ Section 1 — Symptom & Narrative ══════════════════════════════ */}
      <section className="space-y-4">
        <SectionHeader icon={<AlertCircle className="h-4 w-4" />} title="Symptôme & Narrative" />

        <div className="space-y-2">
          <Label htmlFor="wo-symptom">Symptôme observé</Label>
          <Select
            value={symptomId !== null ? String(symptomId) : "none"}
            onValueChange={(v) => setSymptomId(v === "none" ? null : Number(v))}
            disabled={!isDetailEditable}
          >
            <SelectTrigger id="wo-symptom">
              <SelectValue placeholder="Sélectionner un symptôme…" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">— Aucun —</SelectItem>
              {symptoms.map((s) => (
                <SelectItem key={s.id} value={String(s.id)}>
                  {s.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="space-y-2">
          <Label htmlFor="wo-narrative">Description / Narrative</Label>
          <Textarea
            id="wo-narrative"
            placeholder="Décrire la défaillance observée…"
            value={narrative}
            onChange={(e) => setNarrative(e.target.value)}
            disabled={!isDetailEditable}
            rows={3}
          />
        </div>
      </section>

      {/* ══ Section 2 — Failure Analysis ═════════════════════════════════ */}
      <section className="space-y-4">
        <SectionHeader icon={<WrenchIcon className="h-4 w-4" />} title="Analyse de défaillance" />

        <div className="flex items-center gap-2">
          <Checkbox
            id="wo-cause-nd"
            checked={causeNotDetermined}
            onCheckedChange={(v) => setCauseNotDetermined(Boolean(v))}
            disabled={!isDetailEditable}
          />
          <Label htmlFor="wo-cause-nd" className="cursor-pointer">
            Cause non déterminée
          </Label>
        </div>

        <div className="grid gap-4 sm:grid-cols-3">
          <div className="space-y-2">
            <Label htmlFor="wo-failure-mode">Mode de défaillance</Label>
            <Select
              value={failureModeId !== null ? String(failureModeId) : "none"}
              onValueChange={(v) => setFailureModeId(v === "none" ? null : Number(v))}
              disabled={!isDetailEditable || causeNotDetermined}
            >
              <SelectTrigger id="wo-failure-mode">
                <SelectValue placeholder="Mode…" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">— Aucun —</SelectItem>
                {failureModes.map((m) => (
                  <SelectItem key={m.id} value={String(m.id)}>
                    {m.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <Label htmlFor="wo-failure-cause">Cause</Label>
            <Select
              value={failureCauseId !== null ? String(failureCauseId) : "none"}
              onValueChange={(v) => setFailureCauseId(v === "none" ? null : Number(v))}
              disabled={!isDetailEditable || causeNotDetermined}
            >
              <SelectTrigger id="wo-failure-cause">
                <SelectValue placeholder="Cause…" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">— Aucune —</SelectItem>
                {failureCauses.map((c) => (
                  <SelectItem key={c.id} value={String(c.id)}>
                    {c.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="space-y-2">
            <Label htmlFor="wo-failure-effect">Effet</Label>
            <Select
              value={failureEffectId !== null ? String(failureEffectId) : "none"}
              onValueChange={(v) => setFailureEffectId(v === "none" ? null : Number(v))}
              disabled={!isDetailEditable || causeNotDetermined}
            >
              <SelectTrigger id="wo-failure-effect">
                <SelectValue placeholder="Effet…" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">— Aucun —</SelectItem>
                {failureEffects.map((e) => (
                  <SelectItem key={e.id} value={String(e.id)}>
                    {e.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>
      </section>

      {/* ══ Section 3 — Action Performed ═════════════════════════════════ */}
      <section className="space-y-4">
        <SectionHeader icon={<Save className="h-4 w-4" />} title="Action réalisée" />

        <div className="space-y-2">
          <Label htmlFor="wo-corrective-action">Action corrective</Label>
          <Textarea
            id="wo-corrective-action"
            placeholder="Décrire les actions correctives effectuées…"
            value={correctiveAction}
            onChange={(e) => setCorrectiveAction(e.target.value)}
            disabled={!isDetailEditable}
            rows={3}
          />
        </div>

        <div className="space-y-2">
          <Label htmlFor="wo-root-cause">Cause racine</Label>
          <Textarea
            id="wo-root-cause"
            placeholder="Résumé de la cause racine identifiée…"
            value={rootCauseSummary}
            onChange={(e) => setRootCauseSummary(e.target.value)}
            disabled={!isDetailEditable}
            rows={3}
          />
        </div>

        <div className="space-y-2">
          <Label>Type de réparation</Label>
          <div className="flex gap-4">
            {(["temporary", "permanent", "na"] as const).map((val) => (
              <label key={val} className="flex cursor-pointer items-center gap-1.5">
                <input
                  type="radio"
                  name="repair-type"
                  value={val}
                  checked={repairType === val}
                  onChange={() => setRepairType(val)}
                  disabled={!isDetailEditable}
                  className="accent-primary"
                />
                <span className="text-sm">
                  {val === "temporary" ? "Temporaire" : val === "permanent" ? "Permanente" : "S/O"}
                </span>
              </label>
            ))}
          </div>
        </div>

        <div className="space-y-2">
          <Label htmlFor="wo-service-cost">Coût de service (€)</Label>
          <Input
            id="wo-service-cost"
            type="number"
            min={0}
            step={0.01}
            placeholder="0.00"
            value={serviceCost}
            onChange={(e) => setServiceCost(e.target.value)}
            disabled={!isDetailEditable}
            className="w-40"
          />
        </div>

        {/* Save S1-S3 */}
        {isDetailEditable && (
          <div className="space-y-2">
            {detailError && <ErrorBanner message={detailError} />}
            {detailSuccess && <SuccessBanner message="Détails enregistrés." />}
            <Button onClick={() => void handleSaveDetails()} disabled={savingDetails}>
              {savingDetails ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Save className="mr-2 h-4 w-4" />
              )}
              Enregistrer les détails
            </Button>
          </div>
        )}

        {/* Cost summary card */}
        <WoCostSummaryCard woId={wo.id} status={statusCode} />
      </section>
      <section className="space-y-4">
        <SectionHeader icon={<ShieldCheck className="h-4 w-4" />} title="Retour en service" />

        {/* 4a — Mechanical completion */}
        {MECH_COMPLETABLE_STATUSES.has(statusCode) && (
          <div className="space-y-2">
            <p className="text-sm text-muted-foreground">
              Marquer l'OT comme mécaniquement complet lorsque les travaux physiques sont terminés.
            </p>
            {mechError && <ErrorBanner message={mechError} />}
            <Button
              variant="outline"
              onClick={handleMechComplete}
              disabled={!canEdit || completingMech}
            >
              {completingMech ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <WrenchIcon className="mr-2 h-4 w-4" />
              )}
              Marquer mécaniquement complet
            </Button>
          </div>
        )}

        {/* 4b — Technical verification form */}
        {VERIFIABLE_STATUSES.has(statusCode) && (
          <div className="space-y-4 rounded-md border border-surface-border p-4">
            <h4 className="text-sm font-medium text-text-primary">Vérification technique</h4>

            <div className="space-y-2">
              <Label>Résultat</Label>
              <div className="flex gap-4">
                {(
                  [
                    { value: "pass", label: "Approuvé", cls: "text-green-700" },
                    { value: "fail", label: "Refusé", cls: "text-destructive" },
                    { value: "monitor", label: "Surveiller", cls: "text-amber-600" },
                  ] as const
                ).map(({ value, label, cls }) => (
                  <label key={value} className="flex cursor-pointer items-center gap-1.5">
                    <input
                      type="radio"
                      name="verification-result"
                      value={value}
                      checked={verificationResult === value}
                      onChange={() => setVerificationResult(value)}
                      disabled={!canEdit || savingVerification}
                      className="accent-primary"
                    />
                    <span className={`text-sm ${cls}`}>{label}</span>
                  </label>
                ))}
              </div>
            </div>

            {verificationResult === "pass" && (
              <div className="flex items-center gap-2">
                <Checkbox
                  id="wo-rts-confirmed"
                  checked={returnToServiceConfirmed}
                  onCheckedChange={(v) => setReturnToServiceConfirmed(Boolean(v))}
                  disabled={!canEdit || savingVerification}
                />
                <Label htmlFor="wo-rts-confirmed" className="cursor-pointer">
                  Retour en service confirmé
                </Label>
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="wo-ver-notes">Notes de vérification</Label>
              <Textarea
                id="wo-ver-notes"
                placeholder="Observations, tests effectués…"
                value={verificationNotes}
                onChange={(e) => setVerificationNotes(e.target.value)}
                disabled={!canEdit || savingVerification}
                rows={2}
              />
            </div>

            {verificationError && <ErrorBanner message={verificationError} />}

            <Button
              onClick={handleSaveVerification}
              disabled={
                !canEdit ||
                savingVerification ||
                !verificationResult ||
                (verificationResult === "pass" && !returnToServiceConfirmed)
              }
            >
              {savingVerification ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <ShieldCheck className="mr-2 h-4 w-4" />
              )}
              Soumettre vérification
            </Button>
          </div>
        )}

        {/* 4c — Closure */}
        {CLOSEABLE_STATUSES.has(statusCode) && (
          <div className="space-y-4 rounded-md border border-surface-border p-4">
            <h4 className="text-sm font-medium text-text-primary">Fermeture de l'OT</h4>
            <p className="text-sm text-muted-foreground">
              La fermeture est irréversible (sauf via le flux de réouverture supervisé). Une
              authentification renforcée (PIN) est requise.
            </p>

            {/* Step-up PIN — button stays disabled until filled */}
            <div className="space-y-1">
              <Label htmlFor="wo-close-pin">Code PIN (authentification renforcée)</Label>
              <Input
                id="wo-close-pin"
                type="password"
                placeholder="Saisir le PIN…"
                value={stepUpPin}
                onChange={(e) => setStepUpPin(e.target.value)}
                disabled={!canEdit || closing}
                className="w-48"
                autoComplete="current-password"
              />
            </div>

            {/* Blocking errors from preflight */}
            {blockingErrors.length > 0 && (
              <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3">
                <p className="mb-2 text-sm font-medium text-destructive">Conditions bloquantes :</p>
                <ul className="space-y-1">
                  {blockingErrors.map((err, i) => (
                    <li key={i} className="flex items-start gap-2 text-sm text-destructive">
                      <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
                      <span>{err}</span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {closeError && <ErrorBanner message={closeError} />}

            <Button
              variant="destructive"
              onClick={handleClose}
              disabled={!canEdit || closing || stepUpPin.trim() === ""}
            >
              {closing ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Lock className="mr-2 h-4 w-4" />
              )}
              Fermer l'OT
            </Button>
          </div>
        )}
      </section>
    </div>
  );
}
