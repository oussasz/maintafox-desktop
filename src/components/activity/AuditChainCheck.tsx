/**
 * Integrity check of the audit trail: verifies the hash chain of every device whose events this
 * database holds (`verify_audit_chain`) and says where a chain breaks.
 */

import { ShieldAlert, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { describeAppError } from "@/lib/app-error";
import { type AuditChainReport, verifyAuditChain } from "@/services/activity-service";

export function AuditChainCheck() {
  const { t } = useTranslation("activity");
  const [report, setReport] = useState<AuditChainReport | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setRunning(true);
    setError(null);
    try {
      setReport(await verifyAuditChain());
    } catch (e) {
      setReport(null);
      setError(describeAppError(e).title);
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="rounded-md border p-2 text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-muted-foreground">{t("chain.help")}</span>
        <Button size="sm" variant="outline" disabled={running} onClick={() => void run()}>
          {running ? t("chain.running") : t("chain.verify")}
        </Button>
      </div>
      {error ? (
        <p role="alert" className="mt-2 text-destructive">
          {t("chain.failed", { message: error })}
        </p>
      ) : null}
      {report ? (
        <div className="mt-2 space-y-1" data-testid="audit-chain-report">
          <p className={report.intact ? "text-tone-success-fg" : "font-medium text-tone-danger-fg"}>
            {report.intact ? (
              <ShieldCheck className="mr-1 inline h-3 w-3" />
            ) : (
              <ShieldAlert className="mr-1 inline h-3 w-3" />
            )}
            {report.intact ? t("chain.intact") : t("chain.broken")}
          </p>
          <ul className="space-y-0.5">
            {report.chains.map((c) => (
              <li key={c.origin}>
                {t(c.is_this_device ? "chain.thisDevice" : "chain.otherDevice", {
                  code: c.device_code,
                })}
                {" — "}
                {t("chain.events", { count: c.events })}
                {c.first_problem
                  ? ` — ${t(`chain.problem.${c.first_problem.kind}`, {
                      seq: c.first_problem.chain_seq,
                      id: c.first_problem.audit_event_id,
                    })}`
                  : ""}
              </li>
            ))}
          </ul>
          {report.chains.length === 0 ? <p>{t("chain.none")}</p> : null}
          {report.unchained_events > 0 ? (
            <p className="text-muted-foreground">
              {t("chain.unchained", { count: report.unchained_events })}
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
