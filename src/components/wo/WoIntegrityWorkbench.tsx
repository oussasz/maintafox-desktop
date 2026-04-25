/**
 * Data integrity operator workbench (gap 06 sprint 02).
 */

import { AlertTriangle, RefreshCw, Shield } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useSession } from "@/hooks/use-session";
import { listAnalyticsContractVersions } from "@/services/analytics-contract-service";
import {
  applyDataIntegrityRepair,
  listDataIntegrityFindings,
  runDataIntegrityDetectors,
  waiveDataIntegrityFinding,
} from "@/services/data-integrity-service";
import type { AnalyticsContractVersionRow, DataIntegrityFindingRow } from "@shared/ipc-types";

function repairOptionsForCode(code: string): { value: string; labelKey: string }[] {
  switch (code) {
    case "FK_ORPHAN_FAILURE_MODE":
      return [{ value: "clear_failure_mode", labelKey: "integrity.repair.clearFailureMode" }];
    case "WO_DOWNTIME_NEGATIVE_DURATION":
      return [{ value: "swap_downtime_times", labelKey: "integrity.repair.swapDowntime" }];
    case "WO_DOWNTIME_UNCLOSED":
      return [{ value: "close_downtime_at_start", labelKey: "integrity.repair.closeAtStart" }];
    case "WO_DOWNTIME_OVERLAP":
      return [{ value: "trim_overlap_end", labelKey: "integrity.repair.trimOverlap" }];
    default:
      return [];
  }
}

export function WoIntegrityWorkbench() {
  const { t } = useTranslation("ot");
  const { info } = useSession();
  const userId = info?.user_id ?? null;

  const [rows, setRows] = useState<DataIntegrityFindingRow[]>([]);
  const [contracts, setContracts] = useState<AnalyticsContractVersionRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [r, c] = await Promise.all([
        listDataIntegrityFindings(200),
        listAnalyticsContractVersions(),
      ]);
      setRows(r);
      setContracts(c);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const onRunDetectors = useCallback(async () => {
    setRunning(true);
    setError(null);
    try {
      await runDataIntegrityDetectors();
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  }, [load]);

  return (
    <PermissionGate permission="integrity.repair">
      <div className="border-b border-surface-border bg-surface-elevated/40 px-6 py-3">
        <div className="flex items-center justify-between gap-4 flex-wrap">
          <div className="flex items-center gap-2 min-w-0">
            <Shield className="h-4 w-4 text-text-muted shrink-0" />
            <span className="text-sm font-medium text-text-primary">{t("integrity.title")}</span>
            <Badge variant="secondary" className="text-[10px]">
              {rows.length}
            </Badge>
          </div>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            disabled={running || loading}
            onClick={() => void onRunDetectors()}
          >
            <RefreshCw className={`h-3.5 w-3.5 ${running ? "animate-spin" : ""}`} />
            {t("integrity.runDetectors")}
          </Button>
        </div>
        {error && (
          <div className="mt-2 flex items-start gap-2 text-xs text-destructive">
            <AlertTriangle className="h-3.5 w-3.5 mt-0.5 shrink-0" />
            <span className="break-all">{error}</span>
          </div>
        )}
        {contracts[0] && (
          <p className="mt-2 text-[10px] text-text-muted font-mono">
            {t("integrity.contractFrozen", {
              id: contracts[0].contract_id,
              ver: contracts[0].version_semver,
              hash: contracts[0].content_sha256.slice(0, 12),
            })}
          </p>
        )}
        <div className="mt-3 space-y-2 max-h-[220px] overflow-auto">
          {rows.length === 0 && !loading && (
            <p className="text-xs text-text-muted">{t("integrity.empty")}</p>
          )}
          {rows.map((r) => (
            <FindingRow key={r.id} row={r} currentUserId={userId} onRefresh={() => void load()} />
          ))}
        </div>
      </div>
    </PermissionGate>
  );
}

function FindingRow({
  row,
  currentUserId,
  onRefresh,
}: {
  row: DataIntegrityFindingRow;
  currentUserId: number | null;
  onRefresh: () => void;
}) {
  const { t } = useTranslation("ot");
  const [repairKind, setRepairKind] = useState<string>("");
  const [reason, setReason] = useState("");
  const [approverId, setApproverId] = useState("");
  const [busy, setBusy] = useState(false);

  const opts = useMemo(() => repairOptionsForCode(row.finding_code), [row.finding_code]);

  useEffect(() => {
    setRepairKind(opts[0]?.value ?? "");
  }, [opts]);

  const onWaive = useCallback(async () => {
    setBusy(true);
    try {
      const appr = approverId.trim() ? Number(approverId) : null;
      await waiveDataIntegrityFinding({
        finding_id: row.id,
        expected_row_version: row.row_version,
        reason: reason.trim() || t("integrity.waiverDefaultReason"),
        approver_id: appr,
      });
      setReason("");
      setApproverId("");
      onRefresh();
    } finally {
      setBusy(false);
    }
  }, [approverId, onRefresh, reason, row.id, row.row_version, t]);

  const onApply = useCallback(async () => {
    if (!repairKind) return;
    setBusy(true);
    try {
      await applyDataIntegrityRepair({
        finding_id: row.id,
        expected_row_version: row.row_version,
        repair_kind: repairKind,
      });
      onRefresh();
    } finally {
      setBusy(false);
    }
  }, [onRefresh, repairKind, row.id, row.row_version]);

  const severityClass =
    row.severity === "error" ? "bg-red-100 text-red-800" : "bg-amber-100 text-amber-900";

  return (
    <div className="rounded-md border border-surface-border p-2 text-xs space-y-2 bg-background">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant="outline" className={`text-[10px] border-0 ${severityClass}`}>
          {row.severity}
        </Badge>
        <span className="font-mono text-[10px] text-text-muted">{row.finding_code}</span>
        <span className="text-text-muted">
          {row.domain} · {row.record_class} #{row.record_id}
        </span>
      </div>
      <pre className="text-[10px] bg-surface-muted/50 p-2 rounded overflow-x-auto max-h-32">
        {row.details_json}
      </pre>
      <div className="flex flex-wrap gap-2 items-end">
        {row.severity === "error" && (
          <div className="flex flex-col gap-0.5">
            <label className="text-[10px] text-text-muted">{t("integrity.approverId")}</label>
            <Input
              className="h-7 w-28 text-xs"
              inputMode="numeric"
              value={approverId}
              onChange={(e) => setApproverId(e.target.value)}
              placeholder={t("integrity.approverPlaceholder")}
            />
          </div>
        )}
        <div className="flex flex-col gap-0.5 flex-1 min-w-[160px]">
          <label className="text-[10px] text-text-muted">{t("integrity.waiverReason")}</label>
          <Input
            className="h-7 text-xs"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder={t("integrity.waiverReasonPlaceholder")}
          />
        </div>
        <Button
          size="sm"
          variant="secondary"
          className="h-7"
          disabled={
            busy ||
            (row.severity === "error" &&
              currentUserId != null &&
              Number(approverId) === currentUserId)
          }
          onClick={() => void onWaive()}
        >
          {t("integrity.waive")}
        </Button>
      </div>
      {opts.length > 0 && (
        <div className="flex flex-wrap gap-2 items-end pt-1 border-t border-surface-border">
          <div className="flex flex-col gap-0.5">
            <label className="text-[10px] text-text-muted">{t("integrity.repair")}</label>
            <Select value={repairKind} onValueChange={setRepairKind}>
              <SelectTrigger className="h-7 w-[200px] text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {opts.map((o) => (
                  <SelectItem key={o.value} value={o.value}>
                    {t(o.labelKey)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <Button
            size="sm"
            className="h-7"
            disabled={busy || !repairKind}
            onClick={() => void onApply()}
          >
            {t("integrity.applyRepair")}
          </Button>
        </div>
      )}
    </div>
  );
}
