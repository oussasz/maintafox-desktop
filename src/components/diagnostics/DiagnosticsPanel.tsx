import { AlertTriangle, CheckCircle2, RefreshCw, ShieldCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { mfTone } from "@/design-system/tone";
import { useIntegrityCheck } from "@/hooks/use-integrity-check";
import { cn } from "@/lib/utils";

/** Issue codes of the integrity check that have a translation (`shell:diagnostics.issue.*`). */
const ISSUE_CODES = new Set([
  "INSUFFICIENT_VALUES",
  "MISSING_DOMAIN",
  "MISSING_TABLE",
  "SEED_NOT_APPLIED",
]);

/**
 * Integrity diagnostics body â€” embeddable in Settings (no page shell).
 * A failed check is shown with its cause and a retry; the repair rewrites the reference seed
 * data and asks for confirmation first.
 */
export function DiagnosticsPanel() {
  const { t } = useTranslation("shell");
  const { report, status, error, check, repair } = useIntegrityCheck();
  const [confirmRepair, setConfirmRepair] = useState(false);

  useEffect(() => {
    void check();
  }, [check]);

  const busy = status === "checking" || status === "repairing";
  const tone = report?.is_healthy ? "success" : "warning";

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h3 className="text-base font-semibold text-text-primary">{t("diagnostics.title")}</h3>
        <p className="mt-1 text-sm text-text-muted">{t("diagnostics.subtitle")}</p>
      </div>

      {busy ? (
        <LoadingState
          label={status === "checking" ? t("diagnostics.checking") : t("diagnostics.repairing")}
        />
      ) : null}

      {status === "error" ? (
        <ErrorState
          error={error}
          title={t("diagnostics.errorTitle")}
          onRetry={() => void check()}
          retrying={busy}
        />
      ) : null}

      {report && status === "done" ? (
        <>
          <div
            role="status"
            className={cn("flex items-center gap-3 rounded-lg border p-4", mfTone[tone].panel)}
          >
            {report.is_healthy ? (
              <CheckCircle2 className={cn("h-5 w-5", mfTone[tone].text)} aria-hidden />
            ) : (
              <AlertTriangle className={cn("h-5 w-5", mfTone[tone].text)} aria-hidden />
            )}
            <span className={cn("text-sm font-medium", mfTone[tone].text)}>
              {report.is_healthy ? t("diagnostics.healthy") : t("diagnostics.unhealthy")}
            </span>
          </div>

          <dl className="grid grid-cols-3 gap-3">
            <div className="rounded-lg border border-surface-border bg-surface-2 p-4">
              <dt className="text-xs text-text-muted">{t("diagnostics.seedVersion")}</dt>
              <dd className="font-mono text-lg text-text-primary">
                {report.seed_schema_version ?? "â€”"}
              </dd>
            </div>
            <div className="rounded-lg border border-surface-border bg-surface-2 p-4">
              <dt className="text-xs text-text-muted">{t("diagnostics.domainCount")}</dt>
              <dd className="font-mono text-lg text-text-primary">{report.domain_count}</dd>
            </div>
            <div className="rounded-lg border border-surface-border bg-surface-2 p-4">
              <dt className="text-xs text-text-muted">{t("diagnostics.valueCount")}</dt>
              <dd className="font-mono text-lg text-text-primary">{report.value_count}</dd>
            </div>
          </dl>

          {report.issues.length > 0 ? (
            <div className="space-y-2">
              <h4 className="text-sm font-semibold text-text-primary">
                {t("diagnostics.issuesTitle", { count: report.issues.length })}
              </h4>
              <ul className="space-y-2">
                {report.issues.map((issue, i) => (
                  <li
                    key={`${issue.code}-${issue.subject}-${i}`}
                    className="flex items-start gap-3 rounded-lg border border-surface-border bg-surface-2 p-3"
                  >
                    <AlertTriangle
                      className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone.warning.text)}
                      aria-hidden
                    />
                    <div className="min-w-0">
                      <p className="text-sm text-text-primary">
                        {ISSUE_CODES.has(issue.code)
                          ? t(`diagnostics.issue.${issue.code}` as never)
                          : t("diagnostics.issue.other")}
                        {issue.is_auto_repairable ? (
                          <span className={cn("ml-2 text-xs", mfTone.success.text)}>
                            {t("diagnostics.autoRepairable")}
                          </span>
                        ) : null}
                      </p>
                      <details className="mt-0.5 text-xs text-text-muted">
                        <summary className="cursor-pointer select-none">
                          {t("diagnostics.technicalDetails")}
                        </summary>
                        <p className="mt-1 break-words">
                          {issue.subject} â€” {issue.description}
                        </p>
                      </details>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}

          <div className="flex flex-wrap items-center gap-3 pt-2">
            <Button variant="outline" className="h-11" onClick={() => void check()} disabled={busy}>
              <RefreshCw className="h-4 w-4" aria-hidden />
              {t("diagnostics.recheck")}
            </Button>
            {report.is_recoverable && !report.is_healthy ? (
              <Button className="h-11" onClick={() => setConfirmRepair(true)} disabled={busy}>
                <ShieldCheck className="h-4 w-4" aria-hidden />
                {t("diagnostics.repairBtn")}
              </Button>
            ) : null}
          </div>
        </>
      ) : null}

      <ConfirmDialog
        open={confirmRepair}
        onOpenChange={setConfirmRepair}
        title={t("diagnostics.repairConfirmTitle")}
        description={t("diagnostics.repairConfirmBody")}
        confirmLabel={t("diagnostics.repairBtn")}
        onConfirm={() => repair()}
      />
    </div>
  );
}
