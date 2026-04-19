import { ShieldCheck, RefreshCw, AlertTriangle, CheckCircle2, XCircle } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { useIntegrityCheck } from "@/hooks/use-integrity-check";
import { cn } from "@/lib/utils";

export function DiagnosticsPage() {
  const { t } = useTranslation("shell");
  const { report, status, error, check, repair } = useIntegrityCheck();

  useEffect(() => {
    void check();
  }, [check]);

  const busy = status === "checking" || status === "repairing";

  return (
    <ModulePageShell
      icon={ShieldCheck}
      title={t("diagnostics.title")}
      description={t("diagnostics.subtitle")}
      bodyClassName="mx-auto max-w-3xl space-y-6 p-4"
    >
      {/* Loading state */}
      {busy && (
        <div className="flex items-center gap-3 rounded-lg bg-surface-2 border border-surface-border p-4">
          <RefreshCw className="h-5 w-5 animate-spin text-primary" />
          <span className="text-sm text-text-secondary">
            {status === "checking" ? t("diagnostics.checking") : t("diagnostics.repairing")}
          </span>
        </div>
      )}

      {/* Error state */}
      {status === "error" && error && (
        <div className="flex items-start gap-3 rounded-lg bg-status-error/10 border border-status-error/30 p-4">
          <XCircle className="h-5 w-5 text-status-error mt-0.5" />
          <div>
            <p className="text-sm font-medium text-status-error">{t("diagnostics.errorTitle")}</p>
            <p className="text-xs text-text-muted mt-1">{error}</p>
          </div>
        </div>
      )}

      {/* Report */}
      {report && status === "done" && (
        <>
          {/* Health banner */}
          <div
            className={cn(
              "flex items-center gap-3 rounded-lg border p-4",
              report.is_healthy
                ? "bg-status-success/10 border-status-success/30"
                : "bg-status-warning/10 border-status-warning/30",
            )}
          >
            {report.is_healthy ? (
              <CheckCircle2 className="h-5 w-5 text-status-success" />
            ) : (
              <AlertTriangle className="h-5 w-5 text-status-warning" />
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

          {/* Stats grid */}
          <div className="grid grid-cols-3 gap-3">
            <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
              <p className="text-xs text-text-muted">{t("diagnostics.seedVersion")}</p>
              <p className="text-lg font-mono text-text-primary">
                {report.seed_schema_version ?? "—"}
              </p>
            </div>
            <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
              <p className="text-xs text-text-muted">{t("diagnostics.domainCount")}</p>
              <p className="text-lg font-mono text-text-primary">{report.domain_count}</p>
            </div>
            <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
              <p className="text-xs text-text-muted">{t("diagnostics.valueCount")}</p>
              <p className="text-lg font-mono text-text-primary">{report.value_count}</p>
            </div>
          </div>

          {/* Issues list */}
          {report.issues.length > 0 && (
            <div className="space-y-2">
              <h2 className="text-sm font-semibold text-text-primary">
                {t("diagnostics.issuesTitle", { count: report.issues.length })}
              </h2>
              <ul className="space-y-2">
                {report.issues.map((issue) => (
                  <li
                    key={`${issue.code}-${issue.subject}`}
                    className="flex items-start gap-3 rounded-lg bg-surface-2 border border-surface-border p-3"
                  >
                    <AlertTriangle className="h-4 w-4 text-status-warning mt-0.5 shrink-0" />
                    <div className="min-w-0">
                      <p className="text-sm text-text-primary">
                        <span className="font-mono text-xs text-text-muted">{issue.code}</span>
                        {" — "}
                        {issue.description}
                      </p>
                      <p className="text-xs text-text-muted mt-0.5">
                        {issue.subject}
                        {issue.is_auto_repairable && (
                          <span className="ml-2 text-status-success">
                            {t("diagnostics.autoRepairable")}
                          </span>
                        )}
                      </p>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Actions */}
          <div className="flex items-center gap-3 pt-2">
            <button
              type="button"
              onClick={() => void check()}
              disabled={busy}
              className="inline-flex items-center gap-2 rounded-lg bg-surface-3 border border-surface-border px-4 py-2 text-sm font-medium text-text-primary hover:bg-surface-2 disabled:opacity-50"
            >
              <RefreshCw className="h-4 w-4" />
              {t("diagnostics.recheck")}
            </button>
            {report.is_recoverable && !report.is_healthy && (
              <button
                type="button"
                onClick={() => void repair()}
                disabled={busy}
                className="inline-flex items-center gap-2 rounded-lg bg-primary px-4 py-2 text-sm font-medium text-white hover:bg-primary/90 disabled:opacity-50"
              >
                <ShieldCheck className="h-4 w-4" />
                {t("diagnostics.repairBtn")}
              </button>
            )}
          </div>
        </>
      )}
    </ModulePageShell>
  );
}
