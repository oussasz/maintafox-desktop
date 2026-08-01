/**
 * AuditTimeline.tsx
 *
 * Renders the org change audit timeline as a scannable list.
 * Loads recent events from the backend via the governance store.
 */

import { Clock, Loader2, RefreshCw } from "lucide-react";
import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Timeline } from "@/components/timeline";
import type { TimelineEntry, TimelineTone } from "@/components/timeline";
import { Button } from "@/components/ui/button";
import { useOrgGovernanceStore } from "@/stores/org-governance-store";

export function AuditTimeline() {
  const { t } = useTranslation("org");
  const auditEvents = useOrgGovernanceStore((s) => s.auditEvents);
  const auditLoading = useOrgGovernanceStore((s) => s.auditLoading);
  const loadAuditEvents = useOrgGovernanceStore((s) => s.loadAuditEvents);

  useEffect(() => {
    void loadAuditEvents();
  }, [loadAuditEvents]);

  const entries: TimelineEntry[] = useMemo(
    () =>
      auditEvents.map((event) => {
        const badges: NonNullable<TimelineEntry["badges"]> = [
          {
            label: t(`audit.changeType.${event.change_type}`, {
              defaultValue: event.change_type.replace(/_/g, " "),
            }),
            tone:
              event.apply_result === "blocked"
                ? ("danger" as TimelineTone)
                : ("default" as TimelineTone),
          },
        ];
        if (event.requires_step_up) {
          badges.push({
            label: t("governance.stepUpRequired"),
            tone: "muted" as TimelineTone,
          });
        }
        badges.push({
          label: t(`audit.applyResult.${event.apply_result}`, {
            defaultValue: event.apply_result,
          }),
          tone:
            event.apply_result === "applied"
              ? ("success" as TimelineTone)
              : ("danger" as TimelineTone),
        });

        return {
          id: String(event.id),
          title: t(`audit.entityKind.${event.entity_kind}`, {
            defaultValue: event.entity_kind.replace(/_/g, " "),
          }),
          timestamp: event.changed_at,
          badges,
        };
      }),
    [auditEvents, t],
  );

  return (
    <div className="flex h-full flex-col" data-testid="audit-timeline">
      <div className="flex items-center justify-between border-b border-surface-border px-4 py-3">
        <div className="flex items-center gap-2">
          <Clock className="h-4 w-4 text-text-muted" />
          <h3 className="text-sm font-medium text-text-primary">{t("governance.auditTimeline")}</h3>
        </div>
        <Button
          variant="ghost"
          size="sm"
          onClick={() => void loadAuditEvents()}
          disabled={auditLoading}
          className="h-7 w-7 p-0"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${auditLoading ? "animate-spin" : ""}`} />
        </Button>
      </div>

      {auditLoading && auditEvents.length === 0 && (
        <div className="flex items-center justify-center py-8">
          <Loader2 className="h-5 w-5 animate-spin text-text-muted" />
        </div>
      )}

      {!(auditLoading && auditEvents.length === 0) && (
        <div className="flex-1 overflow-y-auto px-2 py-2">
          <Timeline
            items={entries}
            density="compact"
            empty={t("governance.noAuditEvents")}
            itemTestId="audit-event-row"
            aria-label={t("governance.auditTimeline")}
          />
        </div>
      )}
    </div>
  );
}
