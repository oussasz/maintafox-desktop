/**
 * Per-site rules (Settings › Général › Règles par site): a planning site may lift four-eyes on
 * its reliability decisions. Changed under step-up with a reason; every change is listed with who,
 * when, old → new and why. A site without a rule keeps four-eyes.
 */
import { History, ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { useStepUp } from "@/hooks/use-step-up";
import {
  type SiteSettingEvent,
  type SiteSettingView,
  listSiteSettingEvents,
  listSiteSettings,
  setSiteSetting,
} from "@/services/settings-service";

/** Per-site rule: the proposer of a reliability decision may approve it. */
export const SELF_APPROVAL_SITE_RULE = "reliability.decisions.allow_self_approval";
/** Shortest reason accepted (same rule as the server). */
const MIN_REASON_CHARS = 10;

export function SiteRulesPanel({
  onToast,
}: {
  onToast: (msg: { title: string; variant?: "default" | "destructive" | "success" }) => void;
}) {
  const { t } = useTranslation("settings");
  const { formatDateTime } = useFormatters();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [sites, setSites] = useState<SiteSettingView[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [siteId, setSiteId] = useState<number | null>(null);
  const [events, setEvents] = useState<SiteSettingEvent[] | null>(null);
  const [eventsError, setEventsError] = useState<unknown>(null);
  const [pending, setPending] = useState<boolean | null>(null);
  const [reason, setReason] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setSites(await listSiteSettings(SELF_APPROVAL_SITE_RULE));
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, []);

  const loadEvents = useCallback(async (site: number) => {
    setEventsError(null);
    try {
      setEvents(await listSiteSettingEvents(site, SELF_APPROVAL_SITE_RULE));
    } catch (e) {
      setEvents(null);
      setEventsError(e);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (siteId != null) void loadEvents(siteId);
  }, [siteId, loadEvents]);

  const site = useMemo(
    () => sites?.find((s) => s.site_node_id === siteId) ?? null,
    [sites, siteId],
  );
  const effective = site ? (site.value ?? site.default_value) : false;
  const onOff = (v: boolean) => (v ? t("siteRules.on") : t("siteRules.off"));

  return (
    <Card className="shadow-none">
      <CardHeader className="space-y-1 px-5 py-4">
        <CardTitle className="text-base font-semibold">{t("siteRules.title")}</CardTitle>
        <p className="text-xs text-text-secondary">{t("siteRules.hint")}</p>
      </CardHeader>
      <CardContent className="space-y-3 px-5 pb-4 pt-0">
        {error != null ? (
          <ErrorState
            error={error}
            title={t("siteRules.loadError")}
            onRetry={() => void load()}
            retrying={loading}
          />
        ) : sites == null ? (
          <LoadingState />
        ) : sites.length === 0 ? (
          <EmptyState title={t("siteRules.noSites")} description={t("siteRules.noSitesHint")} />
        ) : (
          <>
            <label className="grid gap-1.5 text-sm sm:max-w-sm">
              <span className="font-medium text-text-primary">{t("siteRules.site")}</span>
              <Select
                value={siteId != null ? String(siteId) : ""}
                onValueChange={(v) => setSiteId(Number(v))}
              >
                <SelectTrigger className="h-9" aria-label={t("siteRules.site")}>
                  <SelectValue placeholder={t("siteRules.pickSite")} />
                </SelectTrigger>
                <SelectContent>
                  {sites.map((s) => (
                    <SelectItem key={s.site_node_id} value={String(s.site_node_id)}>
                      {`${s.site_code} · ${s.site_name}`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            {site ? (
              <div className="space-y-2 rounded-md border border-surface-border p-3">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0 flex-1 space-y-0.5">
                    <label
                      htmlFor="site-rule-self-approval"
                      className="text-sm font-medium text-text-primary"
                    >
                      {t("siteRules.selfApproval.label")}
                    </label>
                    <p className="text-xs text-text-secondary">
                      {t("siteRules.selfApproval.help")}
                    </p>
                    <p className="flex flex-wrap items-center gap-2 text-xs text-text-secondary">
                      {site.value_unreadable ? (
                        <StatusChip
                          tone="danger"
                          label={t("siteRules.unreadable", { value: onOff(site.default_value) })}
                        />
                      ) : site.value == null ? (
                        <span>{t("siteRules.notSet", { value: onOff(site.default_value) })}</span>
                      ) : (
                        <span>
                          {t("siteRules.lastChange", {
                            who: site.updated_by_name ?? t("siteRules.unknownUser"),
                            when: site.updated_at ? formatDateTime(site.updated_at) : "",
                          })}
                        </span>
                      )}
                      {effective ? (
                        <StatusChip
                          tone="warning"
                          icon={ShieldAlert}
                          label={t("siteRules.selfApproval.active")}
                        />
                      ) : null}
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    <Switch
                      id="site-rule-self-approval"
                      checked={effective}
                      onCheckedChange={(checked) => {
                        setReason("");
                        setPending(checked);
                      }}
                    />
                    <span className="text-sm text-text-secondary">{onOff(effective)}</span>
                  </div>
                </div>
                <div className="space-y-1">
                  <p className="flex items-center gap-1 text-xs font-medium text-text-secondary">
                    <History className="h-3.5 w-3.5" aria-hidden />
                    {t("siteRules.history")}
                  </p>
                  {eventsError != null ? (
                    <ErrorState
                      error={eventsError}
                      title={t("siteRules.historyError")}
                      onRetry={() => void loadEvents(site.site_node_id)}
                      className="px-4 py-4"
                    />
                  ) : events == null ? (
                    <p className="text-xs text-text-secondary">{t("page.loading")}</p>
                  ) : events.length === 0 ? (
                    <p className="text-xs text-text-secondary">{t("siteRules.noHistory")}</p>
                  ) : (
                    <ul className="space-y-1 border-l-2 border-surface-border pl-3 text-xs">
                      {events.map((e) => (
                        <li key={e.id} className="text-text-secondary">
                          <span className="text-text-primary">{formatDateTime(e.changed_at)}</span>
                          {" · "}
                          {e.changed_by_name ?? t("siteRules.unknownUser")}
                          {" · "}
                          {e.old_value == null ? t("siteRules.unset") : onOff(e.old_value)} →{" "}
                          {onOff(e.new_value)}
                          {" · "}
                          {e.reason}
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              </div>
            ) : null}
          </>
        )}
      </CardContent>
      <ConfirmDialog
        open={pending != null && site != null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title={t("siteRules.confirmTitle", { site: site?.site_name ?? "" })}
        description={t(pending ? "siteRules.confirmOnBody" : "siteRules.confirmOffBody", {
          site: site?.site_name ?? "",
        })}
        confirmLabel={t("siteRules.confirm")}
        destructive={pending === true}
        confirmDisabled={reason.trim().length < MIN_REASON_CHARS}
        onConfirm={async () => {
          if (pending == null || site == null) return;
          await withStepUp(() =>
            setSiteSetting({
              site_node_id: site.site_node_id,
              key: SELF_APPROVAL_SITE_RULE,
              value: pending,
              reason: reason.trim(),
              expected_row_version: site.row_version,
            }),
          );
          onToast({ title: t("siteRules.saved"), variant: "success" });
          await load();
          await loadEvents(site.site_node_id);
        }}
      >
        <label className="grid gap-1.5 text-sm">
          <span className="text-text-secondary">{t("siteRules.reason")}</span>
          <Textarea rows={3} value={reason} onChange={(e) => setReason(e.target.value)} />
          <span className="text-xs text-text-muted">
            {t("siteRules.reasonHint", { min: MIN_REASON_CHARS })}
          </span>
        </label>
      </ConfirmDialog>
      {StepUpDialogElement}
    </Card>
  );
}
