import { AlertTriangle, BellRing, ChevronRight, Info, Siren } from "lucide-react";
import { type UIEvent, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { resolveNotificationLink } from "@/components/notifications/notification-links";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  NOTIFICATION_DELIVERY_TONE,
  NOTIFICATION_SEVERITY_TONE,
  mfTone,
  toneOf,
} from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  type NotificationSummary,
  acknowledgeNotification,
  listNotifications,
  markNotificationRead,
  snoozeNotification,
} from "@/services/notification-service";

type InboxTab = "all" | "unread" | "escalated" | "snoozed";

const TABS: InboxTab[] = ["all", "unread", "escalated", "snoozed"];
/** Delivery state filter sent to `list_notifications` for each tab. */
const TAB_STATE: Record<InboxTab, string | null> = {
  all: null,
  unread: "unread",
  escalated: "escalated",
  snoozed: "snoozed",
};
const SNOOZE_MINUTES = [60, 240] as const;

interface NotificationInboxProps {
  className?: string;
  /** Unread count may have changed (read / acknowledged / snoozed). */
  onChanged?: () => void;
  /** Called after the user opened a source record (e.g. to close the bell dropdown). */
  onNavigate?: () => void;
}

const PAGE_SIZE = 20;

export function NotificationInbox({ className, onChanged, onNavigate }: NotificationInboxProps) {
  const { t } = useTranslation("notifications");
  const { formatRelativeTime, formatDateTime } = useFormatters();
  const navigate = useNavigate();
  const [tab, setTab] = useState<InboxTab>("all");
  const [rows, setRows] = useState<NotificationSummary[]>([]);
  const [hasMore, setHasMore] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const loadingRef = useRef(false);
  const offsetRef = useRef(0);

  const loadPage = useCallback(
    async (reset: boolean) => {
      if (loadingRef.current) return;
      loadingRef.current = true;
      setLoading(true);
      setError(null);
      try {
        const offset = reset ? 0 : offsetRef.current;
        const state = TAB_STATE[tab];
        const data = await listNotifications({
          limit: PAGE_SIZE,
          offset,
          ...(state ? { delivery_state: state } : {}),
        });
        setRows((prev) => (reset ? data : [...prev, ...data]));
        offsetRef.current = offset + data.length;
        setHasMore(data.length === PAGE_SIZE);
      } catch (err) {
        setError(err);
      } finally {
        loadingRef.current = false;
        setLoading(false);
      }
    },
    [tab],
  );

  useEffect(() => {
    offsetRef.current = 0;
    setRows([]);
    setHasMore(true);
    void loadPage(true);
  }, [loadPage]);

  const onScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      if (!hasMore || loadingRef.current || error) return;
      const el = event.currentTarget;
      if (el.scrollHeight - el.scrollTop - el.clientHeight < 120) {
        void loadPage(false);
      }
    },
    [error, hasMore, loadPage],
  );

  /** Runs an action; a failure is reported by the global IPC toast (`describeAppError`). */
  const runAction = useCallback(
    async (id: number, action: () => Promise<void>) => {
      setBusyId(id);
      try {
        await action();
        await loadPage(true);
        onChanged?.();
      } catch {
        // `invoke` already showed the translated failure; the row stays as it was.
      } finally {
        setBusyId(null);
      }
    },
    [loadPage, onChanged],
  );

  const openSource = useCallback(
    async (row: NotificationSummary) => {
      const link = resolveNotificationLink(row.action_url);
      if (!link) return;
      if (row.delivery_state === "delivered") {
        try {
          await markNotificationRead(row.id);
          onChanged?.();
        } catch {
          // Reported by the global toast; still open the record.
        }
      }
      navigate(link.path);
      onNavigate?.();
    },
    [navigate, onChanged, onNavigate],
  );

  const categoryLabel = (code: string) =>
    t(`category.${code}`, { defaultValue: t("category.other") });
  const stateLabel = (state: string) =>
    t(`deliveryState.${state}`, { defaultValue: t("deliveryState.other") });

  return (
    <div className={className}>
      <Tabs value={tab} onValueChange={(v) => setTab(v as InboxTab)}>
        <TabsList className="grid w-full grid-cols-4" aria-label={t("inbox.tabsAria")}>
          {TABS.map((key) => (
            <TabsTrigger key={key} value={key}>
              {t(`inbox.tabs.${key}`)}
            </TabsTrigger>
          ))}
        </TabsList>
      </Tabs>

      <div
        className="mt-3 max-h-[28rem] overflow-auto rounded-md border border-surface-border"
        onScroll={onScroll}
      >
        {error && rows.length === 0 ? (
          <ErrorState
            className="m-2"
            error={error}
            title={t("inbox.loadError")}
            onRetry={() => void loadPage(true)}
          />
        ) : null}

        {rows.length === 0 && !loading && !error ? (
          <div className="flex h-36 items-center justify-center px-4 text-center text-sm text-text-muted">
            {t(`inbox.empty.${tab}`)}
          </div>
        ) : null}

        <ul className="divide-y divide-surface-border">
          {rows.map((row) => {
            const link = resolveNotificationLink(row.action_url);
            const busy = busyId === row.id;
            const unread = row.delivery_state === "delivered";
            const header = (
              <>
                <SeverityIcon severity={row.severity} />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-1.5">
                    <span
                      className={cn(
                        "min-w-0 truncate text-sm text-text-primary",
                        unread ? "font-semibold" : "font-medium",
                      )}
                    >
                      {row.title}
                    </span>
                    <StatusChip label={categoryLabel(row.category_code)} tone="neutral" />
                    <StatusChip
                      label={stateLabel(row.delivery_state)}
                      tone={toneOf(NOTIFICATION_DELIVERY_TONE, row.delivery_state)}
                      dot
                    />
                  </div>
                  {row.body ? (
                    <p className="mt-1 line-clamp-2 text-sm text-text-muted">{row.body}</p>
                  ) : null}
                </div>
                <time
                  dateTime={row.created_at}
                  title={formatDateTime(row.created_at)}
                  className="whitespace-nowrap text-xs text-text-muted"
                >
                  {formatRelativeTime(row.created_at)}
                </time>
                {link ? (
                  <ChevronRight className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" aria-hidden />
                ) : null}
              </>
            );
            return (
              <li key={row.id} className="space-y-2 p-3">
                {link ? (
                  <button
                    type="button"
                    className="flex w-full items-start gap-2 rounded-md text-start hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    aria-label={t("inbox.openAria", { title: row.title })}
                    onClick={() => void openSource(row)}
                  >
                    {header}
                  </button>
                ) : (
                  <div className="flex items-start gap-2">{header}</div>
                )}

                <div className="flex flex-wrap gap-2 ps-6">
                  {unread ? (
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-9"
                      disabled={busy}
                      onClick={() => void runAction(row.id, () => markNotificationRead(row.id))}
                    >
                      {t("inbox.actions.markRead")}
                    </Button>
                  ) : null}
                  {row.requires_ack && row.delivery_state !== "acknowledged" ? (
                    <Button
                      size="sm"
                      variant="outline"
                      className="h-9"
                      disabled={busy}
                      onClick={() => void runAction(row.id, () => acknowledgeNotification(row.id))}
                    >
                      {t("inbox.actions.acknowledge")}
                    </Button>
                  ) : null}
                  {row.delivery_state !== "acknowledged"
                    ? SNOOZE_MINUTES.map((minutes) => (
                        <Button
                          key={minutes}
                          size="sm"
                          variant="ghost"
                          className="h-9"
                          disabled={busy}
                          onClick={() =>
                            void runAction(row.id, () => snoozeNotification(row.id, minutes))
                          }
                        >
                          {t("inbox.actions.snooze", { hours: minutes / 60 })}
                        </Button>
                      ))
                    : null}
                </div>
              </li>
            );
          })}
        </ul>

        {loading ? <LoadingState className="py-3" label={t("inbox.loading")} /> : null}
        {error && rows.length > 0 ? (
          <div className="flex justify-center p-2">
            <Button size="sm" variant="outline" onClick={() => void loadPage(false)}>
              {t("inbox.retryMore")}
            </Button>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function SeverityIcon({ severity }: { severity: string }) {
  const tone = toneOf(NOTIFICATION_SEVERITY_TONE, severity);
  const Icon =
    severity === "critical"
      ? Siren
      : severity === "error"
        ? AlertTriangle
        : severity === "warning"
          ? BellRing
          : Info;
  return <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone[tone].text)} aria-hidden />;
}
