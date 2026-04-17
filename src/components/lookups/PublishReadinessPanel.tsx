/**
 * PublishReadinessPanel.tsx
 *
 * Banner at the top of ReferenceValueEditor for draft sets. Shows
 * publish-readiness validation (blockers + warnings), impact preview,
 * and the publish action button with step-up awareness.
 *
 * Phase 2 – Sub-phase 03 – File 04 – Sprint S4 (GAP REF-04).
 */

import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  ClipboardCheck,
  Eye,
  Send,
  Shield,
  XCircle,
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
import { useReferenceGovernanceStore } from "@/stores/reference-governance-store";
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
import type { ReferencePublishIssue } from "@shared/ipc-types";

// ── Component ─────────────────────────────────────────────────────────────────

interface PublishReadinessPanelProps {
  setId: number;
  isProtected: boolean;
}

export function PublishReadinessPanel({ setId, isProtected }: PublishReadinessPanelProps) {
  const { t } = useTranslation("reference");

  const readiness = useReferenceGovernanceStore((s) => s.readiness);
  const readinessLoading = useReferenceGovernanceStore((s) => s.readinessLoading);
  const impactSummary = useReferenceGovernanceStore((s) => s.impactSummary);
  const impactLoading = useReferenceGovernanceStore((s) => s.impactLoading);
  const loadReadiness = useReferenceGovernanceStore((s) => s.loadReadiness);
  const loadImpact = useReferenceGovernanceStore((s) => s.loadImpact);
  const publish = useReferenceGovernanceStore((s) => s.publish);
  const loadSetsForDomain = useReferenceManagerStore((s) => s.loadSetsForDomain);

  const [expanded, setExpanded] = useState(true);
  const [impactOpen, setImpactOpen] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  // Load readiness on mount
  useEffect(() => {
    void loadReadiness(setId);
  }, [setId, loadReadiness]);

  const blockers =
    readiness?.issues.filter((i: ReferencePublishIssue) => i.severity === "error") ?? [];
  const warnings =
    readiness?.issues.filter((i: ReferencePublishIssue) => i.severity === "warning") ?? [];
  const canPublish = readiness?.is_ready === true && blockers.length === 0;

  const handlePublish = useCallback(async () => {
    setConfirmOpen(false);
    await publish(setId);
    // Reload sets to reflect new published status
    if (readiness?.domain_id) {
      void loadSetsForDomain(readiness.domain_id);
    }
  }, [publish, setId, readiness?.domain_id, loadSetsForDomain]);

  const handlePreviewImpact = useCallback(() => {
    void loadImpact(setId);
    setImpactOpen(true);
  }, [loadImpact, setId]);

  return (
    <>
      <div className="border-b border-surface-border bg-surface-0">
        {/* Header */}
        <button
          type="button"
          className="flex w-full items-center justify-between px-4 py-2.5 hover:bg-surface-1 transition-colors"
          onClick={() => setExpanded(!expanded)}
        >
          <div className="flex items-center gap-2">
            <ClipboardCheck className="h-4 w-4 text-text-muted" />
            <span className="text-sm font-medium text-text-primary">{t("publish.title")}</span>
            {readinessLoading ? (
              <div className="h-3.5 w-3.5 animate-spin rounded-full border border-surface-3 border-t-primary" />
            ) : canPublish ? (
              <Badge variant="default" className="text-[10px] bg-green-500">
                {t("publish.ready")}
              </Badge>
            ) : (
              <Badge variant="destructive" className="text-[10px]">
                {t("publish.notReady", { count: blockers.length })}
              </Badge>
            )}
          </div>
          {expanded ? (
            <ChevronUp className="h-4 w-4 text-text-muted" />
          ) : (
            <ChevronDown className="h-4 w-4 text-text-muted" />
          )}
        </button>

        {/* Expanded content */}
        {expanded && (
          <div className="px-4 pb-3 space-y-3">
            {/* Blockers */}
            {blockers.length > 0 && (
              <div className="space-y-1">
                <p className="text-xs font-medium text-status-danger">{t("publish.blockers")}</p>
                {blockers.map((issue) => (
                  <div
                    key={issue.message}
                    className="flex items-start gap-1.5 text-xs text-status-danger"
                  >
                    <XCircle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
                    <span>{issue.message}</span>
                  </div>
                ))}
              </div>
            )}

            {/* Warnings */}
            {warnings.length > 0 && (
              <div className="space-y-1">
                <p className="text-xs font-medium text-status-warning">{t("publish.warnings")}</p>
                {warnings.map((issue) => (
                  <div
                    key={issue.message}
                    className="flex items-start gap-1.5 text-xs text-status-warning"
                  >
                    <AlertTriangle className="h-3.5 w-3.5 shrink-0 mt-0.5" />
                    <span>{issue.message}</span>
                  </div>
                ))}
              </div>
            )}

            {/* All clear */}
            {blockers.length === 0 && warnings.length === 0 && readiness && (
              <div className="flex items-center gap-1.5 text-xs text-status-success">
                <CheckCircle2 className="h-3.5 w-3.5" />
                <span>{t("publish.allClear")}</span>
              </div>
            )}

            {/* Actions */}
            <div className="flex items-center gap-2 pt-1">
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 h-7 text-xs"
                onClick={handlePreviewImpact}
                disabled={impactLoading}
              >
                <Eye className="h-3 w-3" />
                {t("publish.previewImpact")}
              </Button>
              <Button
                variant="default"
                size="sm"
                className="gap-1.5 h-7 text-xs"
                onClick={() => setConfirmOpen(true)}
                disabled={!canPublish || readinessLoading}
              >
                {isProtected && <Shield className="h-3 w-3" />}
                <Send className="h-3 w-3" />
                {t("publish.publishSet")}
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* ── Impact preview dialog ───────────────────────────────────────── */}
      <Dialog open={impactOpen} onOpenChange={setImpactOpen}>
        <DialogContent className="max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("publish.impactTitle")}</DialogTitle>
            <DialogDescription>{t("publish.impactDescription")}</DialogDescription>
          </DialogHeader>
          {impactLoading ? (
            <div className="flex items-center justify-center py-8">
              <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
            </div>
          ) : impactSummary ? (
            <div className="space-y-3">
              <p className="text-sm">
                {t("publish.totalAffected", { count: impactSummary.total_affected })}
              </p>
              {impactSummary.dimensions.map((dim) => (
                <div
                  key={dim.module}
                  className="flex items-center justify-between text-sm border-b border-surface-border pb-2"
                >
                  <span className="text-text-secondary">{dim.module}</span>
                  <div className="flex items-center gap-2">
                    <Badge
                      variant={dim.status === "ok" ? "default" : "secondary"}
                      className="text-[10px]"
                    >
                      {dim.status}
                    </Badge>
                    <span className="font-mono text-xs">{dim.affected_count}</span>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-sm text-text-muted">{t("publish.noImpactData")}</p>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={() => setImpactOpen(false)}>
              {t("publish.close")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* ── Publish confirmation dialog ─────────────────────────────────── */}
      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("publish.confirmTitle")}</DialogTitle>
            <DialogDescription>
              {isProtected ? t("publish.confirmProtected") : t("publish.confirmDescription")}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)}>
              {t("publish.cancel")}
            </Button>
            <Button onClick={() => void handlePublish()} disabled={readinessLoading}>
              {isProtected && <Shield className="mr-1.5 h-3.5 w-3.5" />}
              {t("publish.confirm")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
