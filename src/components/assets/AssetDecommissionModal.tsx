/**
 * AssetDecommissionModal.tsx
 *
 * GAP EQ-04: Decommission / Retire modal with dependency analysis.
 * Shows binding dependencies, blocker banner, reason textarea,
 * and target state selector (Retired / Scrapped / Transferred).
 */

import {
  AlertTriangle,
  Ban,
  ClipboardList,
  FileText,
  Loader2,
  Radio,
  RefreshCw,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { decommissionAsset, getAssetBindingSummary } from "@/services/asset-lifecycle-service";
import { toErrorMessage } from "@/utils/errors";
import type { Asset, AssetBindingSummary, DecommissionAssetPayload } from "@shared/ipc-types";

type TargetStatus = DecommissionAssetPayload["target_status"];

interface AssetDecommissionModalProps {
  open: boolean;
  asset: Asset;
  onClose: () => void;
  onDecommissioned: (asset: Asset) => void;
}

interface DependencyRow {
  domain: string;
  count: number | null;
  available: boolean;
  icon: React.ReactNode;
  level: "blocker" | "warning" | "info";
  detail: string;
}

const ICON_CLS = "h-4 w-4 shrink-0";

function buildDependencies(
  summary: AssetBindingSummary,
  t: (key: string, opts?: Record<string, unknown>) => string,
): DependencyRow[] {
  const entry = (
    domain: string,
    field: keyof Omit<AssetBindingSummary, "asset_id">,
    icon: React.ReactNode,
    level: "blocker" | "warning" | "info",
    detail: string,
  ): DependencyRow => ({
    domain,
    count: summary[field].count,
    available: summary[field].status === "available",
    icon,
    level,
    detail,
  });

  return [
    entry(
      t("binding.domains.di"),
      "linked_di_count",
      <ClipboardList className={ICON_CLS} />,
      "blocker",
      t("decommission.blockerDetail"),
    ),
    entry(
      t("binding.domains.wo"),
      "linked_wo_count",
      <Wrench className={ICON_CLS} />,
      "blocker",
      t("decommission.blockerDetail"),
    ),
    entry(
      t("binding.domains.pm"),
      "linked_pm_plan_count",
      <RefreshCw className={ICON_CLS} />,
      "warning",
      t("decommission.pmWarning"),
    ),
    entry(
      t("binding.domains.iot"),
      "linked_iot_signal_count",
      <Radio className={ICON_CLS} />,
      "warning",
      t("decommission.iotWarning"),
    ),
    entry(
      t("binding.domains.document"),
      "linked_document_count",
      <FileText className={ICON_CLS} />,
      "info",
      t("decommission.docInfo"),
    ),
  ];
}

export function AssetDecommissionModal({
  open,
  asset,
  onClose,
  onDecommissioned,
}: AssetDecommissionModalProps) {
  const { t } = useTranslation("equipment");

  const [summary, setSummary] = useState<AssetBindingSummary | null>(null);
  const [loadingSummary, setLoadingSummary] = useState(true);
  const [targetStatus, setTargetStatus] = useState<TargetStatus>("RETIRED");
  const [reason, setReason] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSummary = useCallback(async () => {
    setLoadingSummary(true);
    try {
      const data = await getAssetBindingSummary(asset.id);
      setSummary(data);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setLoadingSummary(false);
    }
  }, [asset.id]);

  useEffect(() => {
    if (open) {
      setReason("");
      setTargetStatus("RETIRED");
      setError(null);
      void loadSummary();
    }
  }, [open, loadSummary]);

  const dependencies = summary
    ? buildDependencies(summary, t as (key: string, opts?: Record<string, unknown>) => string)
    : [];
  const blockerCount = dependencies
    .filter((d) => d.level === "blocker" && d.available && d.count !== null && d.count > 0)
    .reduce((acc, d) => acc + (d.count ?? 0), 0);
  const hasBlockers = blockerCount > 0;
  const canConfirm = !hasBlockers && reason.trim().length > 0 && !submitting;

  const handleConfirm = async () => {
    setSubmitting(true);
    setError(null);
    try {
      const result = await decommissionAsset({
        asset_id: asset.id,
        target_status: targetStatus,
        reason: reason.trim(),
        notes: null,
      });
      onDecommissioned(result);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("decommission.title")}</DialogTitle>
          <DialogDescription>{t("decommission.description")}</DialogDescription>
        </DialogHeader>

        {/* Asset identity header */}
        <div className="flex items-center gap-2 rounded-md border p-3">
          <div className="flex-1 space-y-0.5">
            <p className="text-sm font-medium">
              <span className="font-mono">{asset.asset_code}</span> {asset.asset_name}
            </p>
            <p className="text-xs text-text-muted">{asset.class_name ?? "—"}</p>
          </div>
          <Badge variant="outline" className="text-xs">
            {asset.status_code}
          </Badge>
        </div>

        {/* Dependencies */}
        {loadingSummary ? (
          <div className="flex items-center justify-center py-4">
            <Loader2 className="h-4 w-4 animate-spin text-text-muted" />
          </div>
        ) : (
          <div className="space-y-3">
            {/* Blocker banner */}
            {hasBlockers && (
              <div className="flex items-start gap-2 rounded-md border border-status-danger/30 bg-status-danger/5 p-3">
                <Ban className="mt-0.5 h-4 w-4 shrink-0 text-status-danger" />
                <p className="text-sm text-status-danger">
                  {t("decommission.blockerBanner", { count: blockerCount })}
                </p>
              </div>
            )}

            {/* Dependency table */}
            <div className="space-y-1.5">
              {dependencies.map((dep) => {
                const count = !dep.available ? "—" : (dep.count ?? 0).toString();
                const isActive = dep.available && dep.count !== null && dep.count > 0;
                const levelColor =
                  dep.level === "blocker" && isActive
                    ? "text-status-danger"
                    : dep.level === "warning" && isActive
                      ? "text-status-warning"
                      : "text-text-muted";

                return (
                  <div
                    key={dep.domain}
                    className="flex items-center gap-3 rounded-md border px-3 py-2 text-sm"
                  >
                    {dep.icon}
                    <span className="flex-1">{dep.domain}</span>
                    <span className={`font-mono text-xs ${levelColor}`}>{count}</span>
                    {isActive && <span className={`text-xs ${levelColor}`}>{dep.detail}</span>}
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Target state + reason (only when no blockers) */}
        {!loadingSummary && !hasBlockers && (
          <div className="space-y-3">
            <div className="space-y-1.5">
              <Label>{t("decommission.targetStatus")}</Label>
              <Select
                value={targetStatus}
                onValueChange={(v) => setTargetStatus(v as TargetStatus)}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="RETIRED">{t("decommission.statuses.retired")}</SelectItem>
                  <SelectItem value="SCRAPPED">{t("decommission.statuses.scrapped")}</SelectItem>
                  <SelectItem value="TRANSFERRED">
                    {t("decommission.statuses.transferred")}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1.5">
              <Label>{t("decommission.reasonLabel")}</Label>
              <Textarea
                value={reason}
                onChange={(e) => setReason(e.target.value)}
                placeholder={t("decommission.reasonPlaceholder")}
                maxLength={2000}
                rows={3}
              />
              <p className="text-xs text-text-muted text-right">{reason.length}/2000</p>
            </div>
          </div>
        )}

        {error && <p className="text-sm text-status-danger">{error}</p>}

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={submitting}>
            {t("decommission.cancel")}
          </Button>
          <Button variant="destructive" disabled={!canConfirm} onClick={() => void handleConfirm()}>
            {submitting && <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" />}
            <AlertTriangle className="mr-1.5 h-3.5 w-3.5" />
            {t("decommission.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
