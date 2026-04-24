/**
 * PublishReadinessBanner.tsx
 *
 * Displays the publish-readiness status of a draft org model:
 * - blocking issue count + top issues when blockers exist
 * - publish button (disabled when blockers present)
 * - success feedback after publish
 */

import { AlertTriangle, CheckCircle, Loader2, ShieldAlert, Upload } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useOrgDesignerStore } from "@/stores/org-designer-store";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";

interface PublishReadinessBannerProps {
  draftModelId: number | null;
  /** When false, the banner is hidden and validation is not loaded (e.g. read-only “published” mode). */
  visible?: boolean;
}

export function PublishReadinessBanner({
  draftModelId,
  visible = true,
}: PublishReadinessBannerProps) {
  const { t } = useTranslation("org");
  const validation = useOrgGovernanceStore((s) => s.publishValidation);
  const validationLoading = useOrgGovernanceStore((s) => s.validationLoading);
  const error = useOrgGovernanceStore((s) => s.error);
  const loadPublishValidation = useOrgGovernanceStore((s) => s.loadPublishValidation);
  const publishModel = useOrgGovernanceStore((s) => s.publishModel);
  const loadAuditEvents = useOrgGovernanceStore((s) => s.loadAuditEvents);
  const loadSnapshot = useOrgDesignerStore((s) => s.loadSnapshot);

  useEffect(() => {
    if (draftModelId != null && visible) {
      void loadPublishValidation(draftModelId);
    }
  }, [draftModelId, loadPublishValidation, visible]);

  if (!visible || draftModelId == null) return null;

  const canPublish = validation?.can_publish === true;
  const blockingCount = validation?.blocking_count ?? 0;
  const blockingIssues = validation?.issues.filter((i) => i.severity === "error") ?? [];

  const handlePublish = async () => {
    await publishModel(draftModelId);
    // Refresh snapshot and audit after successful publish
    void loadSnapshot();
    void loadAuditEvents();
  };

  return (
    <div className="mx-6 mt-4 space-y-3">
      {/* Validation banner */}
      {validation && !canPublish && (
        <div
          className="rounded-lg border border-status-danger/30 bg-status-danger/10 p-4"
          data-testid="publish-blockers-banner"
        >
          <div className="flex items-center gap-2 mb-2">
            <ShieldAlert className="h-4 w-4 text-status-danger shrink-0" />
            <span className="text-sm font-medium text-status-danger">
              {t("governance.blockingIssues", { count: blockingCount })}
            </span>
          </div>
          <ul className="space-y-1 pl-6" data-testid="blocking-issues-list">
            {blockingIssues.slice(0, 5).map((issue, idx) => (
              <li key={`${issue.code}-${idx}`} className="text-xs text-text-muted list-disc">
                <Badge variant="outline" className="mr-1.5 text-[10px] px-1 py-0">
                  {issue.code}
                </Badge>
                {issue.message}
              </li>
            ))}
            {blockingIssues.length > 5 && (
              <li className="text-xs text-text-muted italic">
                {t("governance.moreIssues", { count: blockingIssues.length - 5 })}
              </li>
            )}
          </ul>
        </div>
      )}

      {validation && canPublish && (
        <div
          className="rounded-lg border border-status-success/30 bg-status-success/10 p-4 flex items-center gap-2"
          data-testid="publish-ready-banner"
        >
          <CheckCircle className="h-4 w-4 text-status-success shrink-0" />
          <span className="text-sm text-status-success">
            {t("governance.readyToPublish", { remapCount: validation.remap_count })}
          </span>
        </div>
      )}

      {error && (
        <div className="rounded-lg border border-status-danger/30 bg-status-danger/10 p-3 flex items-center gap-2">
          <AlertTriangle className="h-4 w-4 text-status-danger shrink-0" />
          <span className="text-xs text-status-danger">{error}</span>
        </div>
      )}

      {/* Publish action row */}
      <div className="flex items-center gap-3">
        <Button
          size="sm"
          disabled={!canPublish || validationLoading}
          onClick={() => void handlePublish()}
          className="gap-1.5"
          data-testid="publish-button"
        >
          {validationLoading ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Upload className="h-3.5 w-3.5" />
          )}
          {t("governance.publish")}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={validationLoading}
          onClick={() => void loadPublishValidation(draftModelId)}
        >
          {t("governance.revalidate")}
        </Button>
      </div>
    </div>
  );
}
