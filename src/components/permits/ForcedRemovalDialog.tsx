/**
 * "Retrait forcé (travailleur absent)": the person who fitted a lock is absent, so a holder of the
 * permit close right at the permit's site authorises another named person to take it off
 * (OSHA 1910.147(e)(3)(ii) A–C, `force_permit_isolation_removal`). The form asks the person who
 * takes the lock off (never the absent worker, never the authoriser), the reason (at least
 * FORCED_REASON_MIN characters), the attestation that the worker is not on the site and the
 * attempts made to reach them (when, how — the plant's contact methods, Données de référence —
 * and what came of it). The save asks for a step-up re-authentication; every refusal is shown with
 * the server's coded, translated reason.
 */

import { Plus, Trash2 } from "lucide-react";
import { type FormEvent, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState } from "@/components/ui/data-state";
import { DateTimeField } from "@/components/ui/date-time-field";
import { isStepUpCancelled, useStepUp } from "@/hooks/use-step-up";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { appErrorReason } from "@/lib/app-error";
import {
  type PermitIsolationRecord,
  forcePermitIsolationRemoval,
  listPermitContactMethods,
  listPermitLockRemovers,
} from "@/services/permit-service";
import type { PermitHandoverReceiverPage, PermitHandoverRoleOption } from "@shared/ipc-types";

import { localInputToIso, serverTextLength, serverTrim, toLocalInputValue } from "./permit-rules";

/**
 * Shortest reason the server accepts: `MIN_FORCED_REMOVAL_REASON_CHARS` in
 * `src-tauri/src/permit/domain.rs` (counted trimmed, in code points).
 */
export const FORCED_REASON_MIN = 20;

interface AttemptDraft {
  /** `datetime-local` value (local time). */
  at: string;
  method: string;
  result: string;
}

const newAttempt = (): AttemptDraft => ({
  at: toLocalInputValue(new Date()),
  method: "",
  result: "",
});

interface ForcedRemovalDialogProps {
  isolation: PermitIsolationRecord | null;
  onOpenChange: (open: boolean) => void;
  /** Called with the isolation after the forced removal was recorded. */
  onRecorded: (isolation: PermitIsolationRecord) => void;
}

export function ForcedRemovalDialog({
  isolation,
  onOpenChange,
  onRecorded,
}: ForcedRemovalDialogProps) {
  const { t } = useTranslation("permits");
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [methods, setMethods] = useState<PermitHandoverRoleOption[] | null>(null);
  const [methodsError, setMethodsError] = useState<unknown>(null);
  const [removers, setRemovers] = useState<PermitHandoverReceiverPage | null>(null);
  const [removersError, setRemoversError] = useState<unknown>(null);
  const [removerSearch, setRemoverSearch] = useState("");
  const search = useDebouncedValue(removerSearch.trim());
  const [remover, setRemover] = useState("");
  const [reason, setReason] = useState("");
  const [attested, setAttested] = useState(false);
  const [attempts, setAttempts] = useState<AttemptDraft[]>([newAttempt()]);
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isolationId = isolation?.id ?? null;

  useEffect(() => {
    if (isolationId == null) return;
    setRemover("");
    setReason("");
    setAttested(false);
    setAttempts([newAttempt()]);
    setSubmitted(false);
    setError(null);
    setRemoverSearch("");
    let cancelled = false;
    listPermitContactMethods()
      .then((m) => {
        if (!cancelled) setMethods(m);
      })
      .catch((e: unknown) => {
        if (!cancelled) setMethodsError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [isolationId]);

  useEffect(() => {
    if (isolationId == null) return;
    let cancelled = false;
    setRemoversError(null);
    listPermitLockRemovers(isolationId, search || null)
      .then((page) => {
        if (!cancelled) setRemovers(page);
      })
      .catch((e: unknown) => {
        if (!cancelled) setRemoversError(e);
      });
    return () => {
      cancelled = true;
    };
  }, [isolationId, search]);

  const reasonLength = serverTextLength(reason);
  const reasonTooShort = reasonLength < FORCED_REASON_MIN;
  const attemptsComplete = attempts.every(
    (a) => localInputToIso(a.at) != null && a.method !== "" && serverTrim(a.result) !== "",
  );
  const valid =
    remover !== "" && !reasonTooShort && attested && attempts.length > 0 && attemptsComplete;

  const setAttempt = (i: number, patch: Partial<AttemptDraft>) =>
    setAttempts((list) => list.map((a, j) => (j === i ? { ...a, ...patch } : a)));

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    setError(null);
    if (!isolation || !valid) return;
    setBusy(true);
    try {
      const saved = await withStepUp(() =>
        forcePermitIsolationRemoval({
          isolation_id: isolation.id,
          expected_row_version: isolation.row_version,
          removed_by_id: Number(remover),
          reason: serverTrim(reason),
          worker_absent_attested: attested,
          contact_attempts: attempts.map((a) => ({
            at: localInputToIso(a.at) ?? a.at,
            method: a.method,
            result: serverTrim(a.result),
          })),
        }),
      );
      onRecorded(saved);
      onOpenChange(false);
    } catch (err) {
      if (!isStepUpCancelled(err)) setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  const worker = isolation?.applied_by_name ?? t("isolation.personUnknown");
  return (
    <>
      <Dialog open={isolation != null} onOpenChange={onOpenChange}>
        <DialogContent className="max-w-2xl">
          <form onSubmit={(e) => void handleSubmit(e)} className="space-y-3" noValidate>
            <DialogHeader>
              <DialogTitle>{t("forced.title")}</DialogTitle>
              <DialogDescription>
                {t("forced.description", { point: isolation?.isolation_point ?? "", worker })}
              </DialogDescription>
            </DialogHeader>

            <FormField
              name="forced-remover"
              label={t("forced.remover")}
              required
              description={t("forced.removerHint", { worker })}
              error={submitted && remover === "" ? t("errors.required") : undefined}
            >
              <div className="space-y-1">
                <Input
                  id="forced-remover-search"
                  aria-label={t("forced.removerSearch")}
                  placeholder={t("forced.removerSearch")}
                  value={removerSearch}
                  onChange={(e) => setRemoverSearch(e.target.value)}
                />
                <Select value={remover} onValueChange={setRemover} disabled={removers == null}>
                  <SelectTrigger id="forced-remover" aria-label={t("forced.remover")}>
                    <SelectValue placeholder={t("forced.removerPlaceholder")} />
                  </SelectTrigger>
                  <SelectContent>
                    {(removers?.items ?? []).map((p) => (
                      <SelectItem key={p.id} value={String(p.id)}>
                        {p.display_name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                {removers != null && removers.total > removers.items.length ? (
                  <p className="text-xs text-text-muted">
                    {t("forced.removersPartial", {
                      shown: removers.items.length,
                      total: removers.total,
                    })}
                  </p>
                ) : null}
                {removersError != null ? <ErrorState error={removersError} /> : null}
              </div>
            </FormField>

            <FormField
              name="forced-reason"
              label={t("forced.reason")}
              required
              description={t("forced.reasonCount", {
                length: reasonLength,
                min: FORCED_REASON_MIN,
              })}
              error={
                submitted && reasonTooShort
                  ? t("forced.reasonMin", { min: FORCED_REASON_MIN })
                  : undefined
              }
            >
              <Textarea
                id="forced-reason"
                rows={2}
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </FormField>

            <fieldset className="space-y-2 rounded-md border border-surface-border p-2">
              <legend className="px-1 text-sm font-medium text-text-primary">
                {t("forced.attempts")}
              </legend>
              <p className="text-xs text-text-muted">{t("forced.attemptsHint")}</p>
              {methodsError != null ? <ErrorState error={methodsError} /> : null}
              {methods != null && methods.length === 0 ? (
                <p role="status" className="text-xs text-tone-warning-fg">
                  {t("forced.noMethods")}
                </p>
              ) : null}
              {attempts.map((a, i) => (
                <div
                  key={i}
                  className="grid gap-2 sm:grid-cols-[auto_1fr_2fr_auto]"
                  data-testid="forced-attempt"
                >
                  <DateTimeField
                    aria-label={t("forced.attemptAt", { n: i + 1 })}
                    value={a.at}
                    onChange={(v) => setAttempt(i, { at: v })}
                  />
                  <Select
                    value={a.method}
                    onValueChange={(v) => setAttempt(i, { method: v })}
                    disabled={methods == null}
                  >
                    <SelectTrigger aria-label={t("forced.attemptMethod", { n: i + 1 })}>
                      <SelectValue placeholder={t("forced.methodPlaceholder")} />
                    </SelectTrigger>
                    <SelectContent>
                      {(methods ?? []).map((m) => (
                        <SelectItem key={m.code} value={m.code}>
                          {m.review_required
                            ? t("handover.roleToReview", { label: m.label })
                            : m.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Input
                    aria-label={t("forced.attemptResult", { n: i + 1 })}
                    placeholder={t("forced.resultPlaceholder")}
                    value={a.result}
                    onChange={(e) => setAttempt(i, { result: e.target.value })}
                  />
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    disabled={attempts.length === 1}
                    aria-label={t("forced.removeAttempt", { n: i + 1 })}
                    onClick={() => setAttempts((list) => list.filter((_, j) => j !== i))}
                  >
                    <Trash2 className="h-4 w-4" aria-hidden />
                  </Button>
                </div>
              ))}
              {submitted && !attemptsComplete ? (
                <p className="text-xs text-tone-danger-fg">{t("forced.attemptsIncomplete")}</p>
              ) : null}
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="gap-1"
                onClick={() => setAttempts((list) => [...list, newAttempt()])}
              >
                <Plus className="h-4 w-4" aria-hidden />
                {t("forced.addAttempt")}
              </Button>
            </fieldset>

            <label className="flex items-start gap-2 text-sm">
              <Checkbox
                checked={attested}
                onCheckedChange={setAttested}
                aria-describedby="forced-attest-hint"
              />
              <span>
                {t("forced.attest")}
                <span id="forced-attest-hint" className="block text-xs text-text-muted">
                  {t("forced.attestHint")}
                </span>
              </span>
            </label>
            {submitted && !attested ? (
              <p className="text-xs text-tone-danger-fg">{t("forced.attestRequired")}</p>
            ) : null}

            <p className="text-xs text-text-muted">{t("forced.stepUpHint")}</p>
            {error ? (
              <p role="alert" className="text-sm text-tone-danger-fg">
                {error}
              </p>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
                {t("actions.cancel")}
              </Button>
              <Button type="submit" variant="destructive" disabled={busy}>
                {t("forced.submit")}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
      {StepUpDialogElement}
    </>
  );
}
