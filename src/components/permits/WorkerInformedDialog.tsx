/**
 * "Travailleur informé": a holder of the permit close right at the permit's site records that the
 * worker of a forced removal was told of it (a worker without an account, or told in person) —
 * when and how (the plant's contact methods, Données de référence). The signed-in person is
 * recorded as who told them (`record_permit_worker_informed`). A worker with an account is told by
 * the blocking screen at their next sign-in instead.
 */

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
} from "@/components/ui";
import { ErrorState } from "@/components/ui/data-state";
import { appErrorReason } from "@/lib/app-error";
import {
  type PermitIsolationRecord,
  listPermitContactMethods,
  recordPermitWorkerInformed,
} from "@/services/permit-service";
import type { PermitHandoverRoleOption } from "@shared/ipc-types";

import { localInputToIso, toLocalInputValue } from "./permit-rules";

interface WorkerInformedDialogProps {
  isolation: PermitIsolationRecord | null;
  onOpenChange: (open: boolean) => void;
  onRecorded: (isolation: PermitIsolationRecord) => void;
}

export function WorkerInformedDialog({
  isolation,
  onOpenChange,
  onRecorded,
}: WorkerInformedDialogProps) {
  const { t } = useTranslation("permits");
  const [methods, setMethods] = useState<PermitHandoverRoleOption[] | null>(null);
  const [methodsError, setMethodsError] = useState<unknown>(null);
  const [method, setMethod] = useState("");
  const [at, setAt] = useState(toLocalInputValue(new Date()));
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isolationId = isolation?.id ?? null;

  useEffect(() => {
    if (isolationId == null) return;
    setMethod("");
    setAt(toLocalInputValue(new Date()));
    setSubmitted(false);
    setError(null);
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

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    setError(null);
    const informedAt = localInputToIso(at);
    if (!isolation || method === "" || informedAt == null) return;
    setBusy(true);
    try {
      const saved = await recordPermitWorkerInformed({
        isolation_id: isolation.id,
        expected_row_version: isolation.row_version,
        method,
        informed_at: informedAt,
      });
      onRecorded(saved);
      onOpenChange(false);
    } catch (err) {
      setError(appErrorReason(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={isolation != null} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={(e) => void handleSubmit(e)} className="space-y-3" noValidate>
          <DialogHeader>
            <DialogTitle>{t("informed.title")}</DialogTitle>
            <DialogDescription>
              {t("informed.description", {
                point: isolation?.isolation_point ?? "",
                worker: isolation?.applied_by_name ?? t("isolation.personUnknown"),
              })}
            </DialogDescription>
          </DialogHeader>
          {methodsError != null ? <ErrorState error={methodsError} /> : null}
          <FormField
            name="informed-method"
            label={t("informed.method")}
            required
            error={submitted && method === "" ? t("errors.required") : undefined}
          >
            <Select value={method} onValueChange={setMethod} disabled={methods == null}>
              <SelectTrigger id="informed-method" aria-label={t("informed.method")}>
                <SelectValue placeholder={t("forced.methodPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {(methods ?? []).map((m) => (
                  <SelectItem key={m.code} value={m.code}>
                    {m.review_required ? t("handover.roleToReview", { label: m.label }) : m.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <FormField
            name="informed-at"
            label={t("informed.at")}
            required
            error={submitted && localInputToIso(at) == null ? t("errors.required") : undefined}
          >
            <Input
              id="informed-at"
              type="datetime-local"
              value={at}
              onChange={(e) => setAt(e.target.value)}
            />
          </FormField>
          <p className="text-xs text-text-muted">{t("informed.hint")}</p>
          {error ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t("actions.cancel")}
            </Button>
            <Button type="submit" disabled={busy}>
              {t("informed.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
