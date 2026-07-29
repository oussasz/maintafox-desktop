/**
 * Unified draft workspace status: draft context, publish safety note, and
 * publish-readiness validation in a single banner (replaces stacked alerts).
 */

import {
  CheckCircle,
  Info,
  Loader2,
  ShieldAlert,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { mfAlert } from "@/design-system/tokens";
import { formatOrgValidationIssue } from "@/lib/format-org-validation-issue";
import { cn } from "@/lib/utils";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";
import type { OrgPublishValidationResult } from "@shared/ipc-types";

type DraftBannerVariant = "info" | "success" | "warning" | "error";

interface DraftStatusBannerProps {
  draftModelId: number | null;
  draftVersion: number | null;
  /** When true, a published model exists alongside this draft. */
  hasActivePublished: boolean;
  /** When false, hidden (e.g. published read-only workspace mode). */
  visible?: boolean;
}

function resolveVariant(
  validation: OrgPublishValidationResult | null,
  validationLoading: boolean,
  storeError: string | null,
): DraftBannerVariant {
  if (storeError) return "error";
  if (validationLoading || validation == null) return "info";

  const blockingIssues = validation.issues.filter((i) => i.severity === "error");
  const warningIssues = validation.issues.filter((i) => i.severity === "warning");

  if (blockingIssues.length > 0 || validation.blocking_count > 0) return "error";
  if (warningIssues.length > 0) return "warning";
  if (validation.can_publish) return "success";
  return "info";
}

const variantShell: Record<DraftBannerVariant, string> = {
  info: mfAlert.info,
  success: mfAlert.success,
  warning: mfAlert.warning,
  error: mfAlert.danger,
};

const variantIcon: Record<DraftBannerVariant, typeof Info> = {
  info: Info,
  success: CheckCircle,
  warning: TriangleAlert,
  error: ShieldAlert,
};

const variantIconClass: Record<DraftBannerVariant, string> = {
  info: "text-primary",
  success: "text-status-success",
  warning: "text-status-warning",
  error: "text-status-danger",
};

const variantBadgeClass: Record<DraftBannerVariant, string> = {
  info: "border-primary/30 text-text-primary",
  success: "border-status-success/40 text-status-success",
  warning: "border-status-warning/40 text-status-warning",
  error: "border-status-danger/40 text-status-danger",
};

export function DraftStatusBanner({
  draftModelId,
  draftVersion,
  hasActivePublished,
  visible = true,
}: DraftStatusBannerProps) {
  const { t } = useTranslation("org");
  const validation = useOrgGovernanceStore((s) => s.publishValidation);
  const validationLoading = useOrgGovernanceStore((s) => s.validationLoading);
  const reconcileLoading = useOrgGovernanceStore((s) => s.reconcileLoading);
  const storeError = useOrgGovernanceStore((s) => s.error);
  const loadPublishValidation = useOrgGovernanceStore((s) => s.loadPublishValidation);
  const reconcileDraftLineage = useOrgGovernanceStore((s) => s.reconcileDraftLineage);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);

  useEffect(() => {
    if (draftModelId != null && visible) {
      void loadPublishValidation(draftModelId);
    }
  }, [draftModelId, loadPublishValidation, visible]);

  const variant = useMemo(
    () => resolveVariant(validation, validationLoading, storeError),
    [validation, validationLoading, storeError],
  );

  if (!visible || draftModelId == null) return null;

  const blockingIssues = validation?.issues.filter((i) => i.severity === "error") ?? [];
  const warningIssues = validation?.issues.filter((i) => i.severity === "warning") ?? [];
  const displayIssues = variant === "error" ? blockingIssues : warningIssues;
  const Icon = variantIcon[variant];
  const hasUnmappedLineage = blockingIssues.some(
    (i) => i.code === "UNMAPPED_ACTIVE_NODE_WITH_OPS_REFS",
  );

  const handleRepairLineage = async () => {
    if (draftModelId == null) return;
    await reconcileDraftLineage(draftModelId);
    void loadSnapshot();
  };

  const draftTitle = hasActivePublished
    ? t("lifecycle.modeDraft", { version: draftVersion ?? "—" })
    : t("draftStatus.prePublishTitle", { version: draftVersion ?? "—" });

  const draftExplanation = hasActivePublished
    ? t("lifecycle.draftModeBanner")
    : t("lifecycle.prePublishDraftOnly");

  const statusLabel = (() => {
    if (validationLoading) return t("draftStatus.status.checking");
    switch (variant) {
      case "success":
        return t("draftStatus.status.ready");
      case "warning":
        return t("draftStatus.status.warning");
      case "error":
        return t("draftStatus.status.error");
      default:
        return t("draftStatus.status.info");
    }
  })();

  const validationSummary = (() => {
    if (validationLoading) return t("draftStatus.validating");
    if (storeError) return storeError;
    if (variant === "success" && validation) {
      return t("governance.readyToPublish", { remapCount: validation.remap_count });
    }
    if (variant === "error" && validation) {
      return t("governance.blockingIssues", { count: validation.blocking_count });
    }
    if (variant === "warning" && validation) {
      return t("draftStatus.warningIssues", { count: warningIssues.length });
    }
    return t("draftStatus.awaitingValidation");
  })();

  const bannerTestId =
    variant === "error"
      ? "publish-blockers-banner"
      : variant === "success"
        ? "publish-ready-banner"
        : "draft-status-banner";

  return (
    <div
      className={cn("mx-6 mt-2 shrink-0", variantShell[variant])}
      role="status"
      data-testid={bannerTestId}
      data-variant={variant}
    >
      <div className="flex items-start gap-2.5">
        {validationLoading ? (
          <Loader2 className="h-4 w-4 shrink-0 animate-spin text-text-muted mt-0.5" />
        ) : (
          <Icon className={cn("h-4 w-4 shrink-0 mt-0.5", variantIconClass[variant])} />
        )}
        <div className="flex-1 min-w-0 space-y-1.5">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-text-primary">{draftTitle}</span>
            <Badge variant="outline" className={cn("text-[10px] px-1.5 py-0", variantBadgeClass[variant])}>
              {statusLabel}
            </Badge>
          </div>
          <p className="text-xs text-text-muted leading-relaxed">{draftExplanation}</p>
          <p
            className={cn(
              "text-xs leading-relaxed",
              variant === "success" && "text-status-success",
              variant === "warning" && "text-status-warning",
              variant === "error" && "text-status-danger",
              variant === "info" && "text-text-primary",
            )}
          >
            {validationSummary}
          </p>

          {displayIssues.length > 0 && (
            <ul className="space-y-1 pl-4 pt-0.5" data-testid="blocking-issues-list">
              {displayIssues.slice(0, 5).map((issue, idx) => (
                <li
                  key={`${issue.code}-${idx}`}
                  className="text-xs text-text-muted list-disc"
                  data-issue-code={issue.code}
                >
                  {formatOrgValidationIssue(issue, t)}
                </li>
              ))}
              {displayIssues.length > 5 && (
                <li className="text-xs text-text-muted italic">
                  {t("governance.moreIssues", { count: displayIssues.length - 5 })}
                </li>
              )}
            </ul>
          )}

          {hasUnmappedLineage && (
            <div className="pt-1.5 space-y-1">
              <PermissionGate permission="org.admin">
                <Button
                  size="sm"
                  variant="outline"
                  className="gap-1.5"
                  disabled={reconcileLoading || validationLoading}
                  onClick={() => void handleRepairLineage()}
                  data-testid="repair-draft-lineage"
                >
                  {reconcileLoading ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Wrench className="h-3.5 w-3.5" />
                  )}
                  {t("governance.repairDraftLineage")}
                </Button>
              </PermissionGate>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
