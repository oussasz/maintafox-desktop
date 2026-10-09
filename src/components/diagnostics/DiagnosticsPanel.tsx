import { AlertTriangle, CheckCircle2, RefreshCw, ShieldCheck, XCircle } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { useIntegrityCheck } from "@/hooks/use-integrity-check";
import { cn } from "@/lib/utils";

/**
 * Integrity diagnostics body — embeddable in Settings (no page shell).
 */
export function DiagnosticsPanel() {
  const { t } = useTranslation("shell");
  const { report, status, error, check, repair } = useIntegrityCheck();

  useEffect(() => {
    void check();
  }, [check]);

  const busy = status === "checking" || status === "repairing";

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h3 className="text-base font-semibold text-text-primary">{t("diagnostics.title")}</h3>
        <p className="mt-1 text-sm text-text-muted">{t("diagnostics.subtitle")}</p>
      </div>

      {busy ? (
        <div className="flex items-center gap-3 rounded-lg border border-surface-border bg-surface-2 p-4">
          <RefreshCw className="h-5 w-5 animate-spin text-primary" aria-hidden />
          <span className="text-sm text-text-secondary">
            {status === "checking" ? t("diagnostics.checking") : t("diagnostics.repairing")}
          </span>
        </div>
      ) : null}

      {status === "error" && error ? (
        <div className="flex items-start gap-3 rounded-lg border border-status-error/30 bg-status-error/10 p-4">
          <XCircle className="mt-0.5 h-5 w-5 text-status-error" aria-hidden />
          <div>
            <p className="text-sm font-medium text-status-error">{t("diagnostics.errorTitle")}</p>
            <p className="mt-1 text-xs text-text-muted">{error}</p>
          </div>
        </div>
      ) : null}

      {report && status === "done" ? (
        <>
          <div
            className={cn(
              "flex items-center gap-3 rounded-lg border p-4",
              report.is_healthy
                ? "border-status-success/30 bg-status-success/10"
                : "border-status-warning/30 bg-status-warning/10",
            )}
          >
            {report.is_healthy ? (
              <CheckCircle2 className="h-5 w-5 text-status-success" aria-hidden />
            ) : (
              <AlertTriangle className="h-5 w-5 text-status-warning" aria-hidden />
            )}
            <span
              className={cn(
                "text-sm font-medium",
                report.is_healthy ? "text-status-success" : "text-status-warning",
              )}
            >
              {report.is_healthy ? t("diagnostics.healthy") : t("diagnostics.unhealthy")}
            </span>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div className="rounded-lg border border-surface-border bg-surface-2 p-4">
              <p className="text-xs text-text-muted">{t("diagnostics.seedVersion")}</p>
              <p className="font-mono text-lg text-text-primary">
                {report.seed_schema_version ?? "—"}
              </p>
            </div>
            <div className="rounded-lg border border-surface-border bg-surface-2 p-4">
              <p className="text-xs text-text-muted">{t("diagnostics.domainCount")}</p>
              <p className="font-mono text-lg text-text-primary">{report.domain_count}</p>
            </div>
            <div className="rounded-lg border border-surface-border bg-surface-2 p-4">
              <p className="text-xs text-text-muted">{t("diagnostics.valueCount")}</p>
              <p className="font-mono text-lg text-text-primary">{report.value_count}</p>
            </div>
          </div>

          {report.issues.length > 0 ? (
            <div className="space-y-2">
              <h4 className="text-sm font-semibold text-text-primary">
                {t("diagnostics.issuesTitle", { count: report.issues.length })}
              </h4>
              <ul className="space-y-2">
                {report.issues.map((issue) => (
                  <li
                    key={`${issue.code}-${issue.subject}`}
                    className="flex items-start gap-3 rounded-lg border border-surface-border bg-surface-2 p-3"
                  >
                    <AlertTriangle
                      className="mt-0.5 h-4 w-4 shrink-0 text-status-warning"
                      aria-hidden
                    />
                    <div className="min-w-0">
                      <p className="text-sm text-text-primary">
                        <span className="font-mono text-xs text-text-muted">{issue.code}</span>
                        {" — "}
                        {issue.description}
                      </p>
                      <p className="mt-0.5 text-xs text-text-muted">
                        {issue.subject}
                        {issue.is_auto_repairable ? (
                          <span className="ml-2 text-status-success">
                            {t("diagnostics.autoRepairable")}
                          </span>
                        ) : null}
                      </p>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}

          <div className="flex items-center gap-3 pt-2">
            <button
              type="button"
              onClick={() => void check()}
              disabled={busy}
              className="inline-flex items-center gap-2 rounded-lg border border-surface-border bg-surface-3 px-4 py-2 text-sm font-medium text-text-primary hover:bg-surface-2 disabled:opacity-50"
            >
              <RefreshCw className="h-4 w-4" aria-hidden />
              {t("diagnostics.recheck")}
            </button>
            {report.is_recoverable && !report.is_healthy ? (
              <button
                type="button"
                onClick={() => void repair()}
                disabled={busy}
                className="inline-flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/90 disabled:opacity-50"
              >
                <ShieldCheck className="h-4 w-4" aria-hidden />
                {t("diagnostics.repairBtn")}
              </button>
            ) : null}
          </div>
        </>
      ) : null}
    </div>
  );
}
