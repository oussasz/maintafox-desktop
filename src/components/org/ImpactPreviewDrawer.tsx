/**
 * ImpactPreviewDrawer.tsx
 *
 * Side sheet that displays the impact preview before a structural change
 * (move, deactivate, reassign). Blockers disable the confirm button;
 * warnings require explicit acknowledgement before proceeding.
 */

import {
  AlertTriangle,
  Ban,
  Check,
  ExternalLink,
  GitBranch,
  Loader2,
  Link2,
  ShieldAlert,
  Users,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useOrgDesignerStore } from "@/stores/org-designer-store";

export function ImpactPreviewDrawer() {
  const { t } = useTranslation("org");
  const preview = useOrgDesignerStore((s) => s.preview);
  const previewOpen = useOrgDesignerStore((s) => s.previewOpen);
  const previewLoading = useOrgDesignerStore((s) => s.previewLoading);
  const closePreview = useOrgDesignerStore((s) => s.closePreview);

  const [warningsAcknowledged, setWarningsAcknowledged] = useState(false);

  const hasBlockers = (preview?.blockers.length ?? 0) > 0;
  const hasWarnings = (preview?.warnings.length ?? 0) > 0;
  const confirmDisabled = hasBlockers || (hasWarnings && !warningsAcknowledged);

  const handleOpenChange = (open: boolean) => {
    if (!open) {
      closePreview();
      setWarningsAcknowledged(false);
    }
  };

  const handleConfirm = () => {
    // S3 does not implement the final mutation — that is deferred to the
    // respective action modules. Close the drawer for now.
    closePreview();
    setWarningsAcknowledged(false);
  };

  const actionLabel = preview ? t(`preview.actionLabel.${preview.action}`) : "";

  return (
    <Sheet open={previewOpen} onOpenChange={handleOpenChange}>
      <SheetContent side="right" className="flex flex-col sm:max-w-md">
        <SheetHeader>
          <SheetTitle>{t("preview.title")}</SheetTitle>
          <SheetDescription>{actionLabel}</SheetDescription>
        </SheetHeader>

        {/* Loading */}
        {previewLoading && (
          <div className="flex flex-1 items-center justify-center">
            <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
          </div>
        )}

        {/* Preview content */}
        {preview && !previewLoading && (
          <div className="flex-1 overflow-y-auto space-y-5 py-2">
            {/* Impact summary */}
            <section className="space-y-2">
              <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wider">
                {t("preview.impactSummary")}
              </h4>
              <div className="grid grid-cols-2 gap-2">
                <SummaryCard
                  icon={<GitBranch className="h-4 w-4" />}
                  label={t("preview.affectedNodes")}
                  value={preview.affected_node_count}
                />
                <SummaryCard
                  icon={<GitBranch className="h-4 w-4" />}
                  label={t("preview.descendants")}
                  value={preview.descendant_count}
                />
                <SummaryCard
                  icon={<Users className="h-4 w-4" />}
                  label={t("preview.responsibilities")}
                  value={preview.active_responsibility_count}
                />
                <SummaryCard
                  icon={<Link2 className="h-4 w-4" />}
                  label={t("preview.activeBindings")}
                  value={preview.active_binding_count}
                />
              </div>
            </section>

            {/* Blockers */}
            {hasBlockers && (
              <section className="space-y-2">
                <h4 className="text-xs font-semibold text-status-danger uppercase tracking-wider flex items-center gap-1.5">
                  <Ban className="h-3.5 w-3.5" />
                  {t("preview.blockers")}
                </h4>
                <ul className="space-y-1.5" role="list" aria-label={t("preview.blockers")}>
                  {preview.blockers.map((b, i) => (
                    <li
                      key={i}
                      className="rounded-md border border-status-danger/30 bg-status-danger/10 px-3 py-2 text-sm text-text-primary"
                    >
                      {b}
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {/* Warnings */}
            {hasWarnings && (
              <section className="space-y-2">
                <h4 className="text-xs font-semibold text-status-warning uppercase tracking-wider flex items-center gap-1.5">
                  <AlertTriangle className="h-3.5 w-3.5" />
                  {t("preview.warnings")}
                </h4>
                <ul className="space-y-1.5" role="list" aria-label={t("preview.warnings")}>
                  {preview.warnings.map((w, i) => (
                    <li
                      key={i}
                      className="rounded-md border border-status-warning/30 bg-status-warning/10 px-3 py-2 text-sm text-text-primary"
                    >
                      {w}
                    </li>
                  ))}
                </ul>

                {/* Acknowledge checkbox (only when no blockers) */}
                {!hasBlockers && (
                  <label className="flex items-center gap-2 cursor-pointer pt-1">
                    <input
                      type="checkbox"
                      checked={warningsAcknowledged}
                      onChange={(e) => setWarningsAcknowledged(e.target.checked)}
                      className="rounded border-surface-border"
                    />
                    <span className="text-xs text-text-muted">
                      {t("preview.acknowledgeWarnings")}
                    </span>
                  </label>
                )}
              </section>
            )}

            <Separator />

            {/* Dependency placeholders */}
            <section className="space-y-2">
              <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wider flex items-center gap-1.5">
                <ExternalLink className="h-3.5 w-3.5" />
                {t("preview.dependencies")}
              </h4>
              <div className="space-y-1.5">
                {preview.dependencies.map((dep) => (
                  <div
                    key={dep.domain}
                    className="flex items-center justify-between rounded-md border border-surface-border px-3 py-2"
                  >
                    <div className="space-y-0.5">
                      <span className="text-sm text-text-primary capitalize">{dep.domain}</span>
                      {dep.note && <p className="text-[11px] text-text-muted">{dep.note}</p>}
                    </div>
                    <Badge
                      variant={dep.status === "unavailable" ? "outline" : "secondary"}
                      className="text-[10px] shrink-0"
                    >
                      {dep.status}
                    </Badge>
                  </div>
                ))}
              </div>
            </section>
          </div>
        )}

        {/* Footer */}
        {preview && !previewLoading && (
          <SheetFooter className="border-t border-surface-border pt-4">
            <Button variant="outline" onClick={() => handleOpenChange(false)}>
              {t("preview.cancel")}
            </Button>
            <Button
              disabled={confirmDisabled}
              onClick={handleConfirm}
              className="gap-1.5"
              variant={hasBlockers ? "outline" : "default"}
            >
              {hasBlockers ? (
                <>
                  <ShieldAlert className="h-3.5 w-3.5" />
                  {t("preview.blocked")}
                </>
              ) : (
                <>
                  <Check className="h-3.5 w-3.5" />
                  {t("preview.confirm")}
                </>
              )}
            </Button>
          </SheetFooter>
        )}
      </SheetContent>
    </Sheet>
  );
}

// ── Internal helper ───────────────────────────────────────────────────────────

function SummaryCard({
  icon,
  label,
  value,
}: {
  icon: React.ReactNode;
  label: string;
  value: number;
}) {
  return (
    <div className="rounded-lg border border-surface-border p-3 space-y-1">
      <div className="flex items-center gap-1.5 text-text-muted">
        {icon}
        <span className="text-[11px]">{label}</span>
      </div>
      <p className="text-lg font-semibold text-text-primary">{value}</p>
    </div>
  );
}
