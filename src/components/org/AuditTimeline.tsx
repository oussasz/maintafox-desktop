/**
 * AuditTimeline.tsx
 *
 * Renders the org change audit timeline as a scannable list.
 * Loads recent events from the backend via the governance store.
 */

import { Clock, Loader2, RefreshCw } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
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

  return (
    <div className="flex flex-col h-full" data-testid="audit-timeline">
      <div className="flex items-center justify-between px-4 py-3 border-b border-surface-border">
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

      {!auditLoading && auditEvents.length === 0 && (
        <div className="flex items-center justify-center py-8">
          <p className="text-xs text-text-muted">{t("governance.noAuditEvents")}</p>
        </div>
      )}

      <div className="flex-1 overflow-y-auto">
        {auditEvents.map((event) => (
          <div
            key={event.id}
            className="px-4 py-3 border-b border-surface-border hover:bg-surface-hover transition-colors"
            data-testid="audit-event-row"
          >
            <div className="flex items-center gap-2 mb-1">
              <Badge
                variant={event.apply_result === "blocked" ? "destructive" : "default"}
                className="text-[10px] px-1.5 py-0"
              >
                {event.change_type}
              </Badge>
              {event.requires_step_up && (
                <Badge variant="outline" className="text-[10px] px-1.5 py-0">
                  {t("governance.stepUpRequired")}
                </Badge>
              )}
              <Badge
                variant={event.apply_result === "applied" ? "default" : "destructive"}
                className="text-[10px] px-1.5 py-0"
              >
                {event.apply_result}
              </Badge>
            </div>
            <div className="text-xs text-text-muted">
              <span>{event.entity_kind}</span>
              {event.entity_id != null && <span className="ml-1">#{event.entity_id}</span>}
              <span className="mx-1.5">·</span>
              <time>{new Date(event.changed_at).toLocaleString()}</time>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
