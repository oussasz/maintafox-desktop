/**
 * Notification inbox: the person's notifications, newest first.
 *
 * - `page` variant (Notifications page): views (inbox, unread, to acknowledge, snoozed, archived),
 *   filters by module, severity and creation day, grouping by the record the notifications are
 *   about, "mark all as read" for the current filters.
 * - `compact` variant (bell menu): the unread view, without filters.
 *
 * Each row opens the exact record (`notification-links`), says why the person received it, shows
 * its severity — or the level of the shared urgency scale when it is about work — and offers the
 * actions of its state: mark read / unread, acknowledge, snooze (1 h, 4 h, tomorrow 08:00),
 * archive / restore. Counts come from the backend (`notification-store`) with the same conditions
 * as the list, so the bell and the inbox agree.
 */

import {
  Archive,
  ArchiveRestore,
  BellRing,
  Check,
  CheckCheck,
  ChevronRight,
  Clock,
  Info,
  MailOpen,
  Siren,
} from "lucide-react";
import { type UIEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { urgencyTone, useUrgencyScale } from "@/components/common/urgency/use-urgency-scale";
import {
  OPEN_SYNC_CENTER_EVENT,
  SYNC_CENTER_LINK,
  resolveNotificationLink,
} from "@/components/notifications/notification-links";
import {
  categoryLabel,
  localDayBounds,
  notificationReason,
  notificationText,
  tomorrowMorning,
} from "@/components/notifications/notification-text";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { NOTIFICATION_SEVERITY_TONE, mfTone, toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import {
  INBOX_VIEWS,
  type InboxView,
  type NotificationFilterInput,
  type NotificationSummary,
  acknowledgeNotification,
  archiveNotification,
  listNotifications,
  markAllNotificationsRead,
  markNotificationRead,
  markNotificationUnread,
  snoozeNotification,
  unarchiveNotification,
} from "@/services/notification-service";
import { useNotificationStore } from "@/stores/notification-store";

/** Modules of the notification categories (`notifications::catalog::MODULES`). */
export const NOTIFICATION_MODULES = [
  "di",
  "ot",
  "pm",
  "ins",
  "inv",
  "ptw",
  "per",
  "plan",
  "fin",
  "sync",
  "system",
] as const;
export const NOTIFICATION_SEVERITIES = ["info", "warning", "critical"] as const;

const ALL = "__all__";
const PAGE_SIZE = 25;
const COMPACT_SIZE = 8;

interface NotificationInboxProps {
  variant?: "page" | "compact";
  className?: string;
  /** Called after the user opened a source record (e.g. to close the bell menu). */
  onNavigate?: () => void;
}

interface Filters {
  view: InboxView;
  module: string;
  severity: string;
  from: string;
  to: string;
}

const INITIAL: Filters = { view: "inbox", module: ALL, severity: ALL, from: "", to: "" };

function toFilterInput(f: Filters): NotificationFilterInput {
  const out: NotificationFilterInput = { view: f.view };
  if (f.module !== ALL) out.module_code = f.module;
  if (f.severity !== ALL) out.severity = f.severity;
  const from = f.from ? localDayBounds(f.from, false) : undefined;
  const to = f.to ? localDayBounds(f.to, true) : undefined;
  if (from) out.created_from = from;
  if (to) out.created_to = to;
  return out;
}

function isUnread(row: NotificationSummary): boolean {
  return (
    row.read_at == null &&
    row.acknowledged_at == null &&
    row.resolved_at == null &&
    row.archived_at == null &&
    row.delivery_state !== "snoozed"
  );
}

function needsAck(row: NotificationSummary): boolean {
  return (
    row.requires_ack &&
    row.event_acknowledged_by == null &&
    row.resolved_at == null &&
    row.archived_at == null
  );
}

/** Rows about the same record, in list order. */
function groupByObject(rows: NotificationSummary[]): NotificationSummary[][] {
  const groups = new Map<string, NotificationSummary[]>();
  for (const row of rows) {
    const key =
      row.source_record_id != null
        ? `${row.source_module}:${row.source_record_id}`
        : `notification:${row.id}`;
    const group = groups.get(key);
    if (group) group.push(row);
    else groups.set(key, [row]);
  }
  return [...groups.values()];
}

export function NotificationInbox({
  variant = "page",
  className,
  onNavigate,
}: NotificationInboxProps) {
  const compact = variant === "compact";
  const { t } = useTranslation("notifications");
  const fmt = useFormatters();
  const navigate = useNavigate();
  const urgency = useUrgencyScale();
  const refreshCounts = useNotificationStore((s) => s.refresh);
  const counts = useNotificationStore((s) => s.counts);
  const arrivals = useNotificationStore((s) => s.arrivals);

  const [filters, setFilters] = useState<Filters>(
    compact ? { ...INITIAL, view: "unread" } : INITIAL,
  );
  const [grouped, setGrouped] = useState(false);
  const [rows, setRows] = useState<NotificationSummary[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [bulkBusy, setBulkBusy] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const loadingRef = useRef(false);
  const offsetRef = useRef(0);
  const filterInput = useMemo(() => toFilterInput(filters), [filters]);
  const pageSize = compact ? COMPACT_SIZE : PAGE_SIZE;

  const loadPage = useCallback(
    async (reset: boolean) => {
      if (loadingRef.current) return;
      loadingRef.current = true;
      setLoading(true);
      setError(null);
      try {
        const offset = reset ? 0 : offsetRef.current;
        const data = await listNotifications({ ...filterInput, limit: pageSize, offset });
        setRows((prev) => (reset ? data : [...prev, ...data]));
        offsetRef.current = offset + data.length;
        setHasMore(!compact && data.length === pageSize);
      } catch (err) {
        setError(err);
      } finally {
        loadingRef.current = false;
        setLoading(false);
      }
    },
    [compact, filterInput, pageSize],
  );

  useEffect(() => {
    offsetRef.current = 0;
    void loadPage(true);
  }, [loadPage]);

  // A new notification arrived (the bell's poll saw the unread count go up): reload and announce.
  const seenArrivals = useRef(arrivals);
  useEffect(() => {
    if (arrivals === seenArrivals.current) return;
    seenArrivals.current = arrivals;
    setAnnouncement(t("inbox.newArrivals", { count: counts.unread }));
    offsetRef.current = 0;
    void loadPage(true);
  }, [arrivals, counts.unread, loadPage, t]);

  const onScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      if (!hasMore || loadingRef.current || error) return;
      const el = event.currentTarget;
      if (el.scrollHeight - el.scrollTop - el.clientHeight < 120) void loadPage(false);
    },
    [error, hasMore, loadPage],
  );

  /** Runs an action; a failure is reported by the global IPC toast (`describeAppError`). */
  const runAction = useCallback(
    async (id: number, action: () => Promise<void>) => {
      setBusyId(id);
      try {
        await action();
        offsetRef.current = 0;
        await loadPage(true);
        await refreshCounts();
      } catch {
        // `invoke` already showed the translated failure; the row stays as it was.
      } finally {
        setBusyId(null);
      }
    },
    [loadPage, refreshCounts],
  );

  const markAll = useCallback(async () => {
    setBulkBusy(true);
    try {
      const n = await markAllNotificationsRead(filterInput);
      setAnnouncement(t("inbox.markedAll", { count: n }));
      offsetRef.current = 0;
      await loadPage(true);
      await refreshCounts();
    } catch {
      // Reported by the global toast.
    } finally {
      setBulkBusy(false);
    }
  }, [filterInput, loadPage, refreshCounts, t]);

  const openSource = useCallback(
    async (row: NotificationSummary) => {
      const link = resolveNotificationLink(row.action_url);
      if (!link) return;
      if (isUnread(row)) {
        try {
          await markNotificationRead(row.id);
          void refreshCounts();
        } catch {
          // Reported by the global toast; still open the record.
        }
      }
      if (link.path === SYNC_CENTER_LINK) {
        window.dispatchEvent(new CustomEvent(OPEN_SYNC_CENTER_EVENT));
      } else {
        navigate(link.path);
      }
      onNavigate?.();
    },
    [navigate, onNavigate, refreshCounts],
  );

  const setFilter = (patch: Partial<Filters>) => setFilters((f) => ({ ...f, ...patch }));
  const groups = grouped && !compact ? groupByObject(rows) : rows.map((r) => [r]);
  const filtered =
    filters.module !== ALL || filters.severity !== ALL || filters.from !== "" || filters.to !== "";

  const renderRow = (row: NotificationSummary, nested: boolean) => {
    const link = resolveNotificationLink(row.action_url);
    const busy = busyId === row.id;
    const unread = isUnread(row);
    const text = notificationText(row, t, fmt);
    const reason = notificationReason(row, t);
    const header = (
      <>
        <SeverityIcon severity={row.severity} />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <span
              className={cn(
                "min-w-0 text-sm text-text-primary",
                unread ? "font-semibold" : "font-medium",
              )}
            >
              {unread ? <span className="sr-only">{t("inbox.unreadMark")} </span> : null}
              {text.title}
            </span>
          </div>
          {text.body ? (
            <p className="mt-0.5 line-clamp-2 text-sm text-text-muted">{text.body}</p>
          ) : null}
          <div className="mt-1 flex flex-wrap items-center gap-1.5">
            <StatusChip label={t(`module.${row.module_code}`)} tone="neutral" />
            {!nested ? (
              <StatusChip label={categoryLabel(row.category_code, t)} tone="neutral" />
            ) : null}
            {row.urgency_code ? (
              <StatusChip
                label={urgency.labelOf(row.urgency_code)}
                tone={urgencyTone(row.urgency_code)}
                dot
                title={t("inbox.urgencyTitle")}
              />
            ) : (
              <StatusChip
                label={t(`severity.${row.severity}`)}
                tone={toneOf(NOTIFICATION_SEVERITY_TONE, row.severity)}
                dot
              />
            )}
            {row.escalation_level > 0 ? (
              <StatusChip
                label={t("inbox.escalated", { level: row.escalation_level })}
                tone="attention"
              />
            ) : null}
            {row.resolved_at ? <StatusChip label={t("inbox.resolved")} tone="success" /> : null}
            {row.event_acknowledged_by != null ? (
              <StatusChip
                label={
                  row.event_acknowledged_by
                    ? t("inbox.acknowledgedBy", { name: row.event_acknowledged_by })
                    : t("inbox.acknowledged")
                }
                tone="success"
              />
            ) : needsAck(row) ? (
              <StatusChip label={t("inbox.ackRequired")} tone="attention" />
            ) : null}
            {row.delivery_state === "snoozed" && row.snoozed_until ? (
              <StatusChip
                label={t("inbox.snoozedUntil", { when: fmt.formatDateTime(row.snoozed_until) })}
                tone="neutral"
              />
            ) : null}
          </div>
          {reason ? <p className="mt-1 text-xs text-text-muted">{reason}</p> : null}
        </div>
        <time
          dateTime={row.created_at}
          title={fmt.formatDateTime(row.created_at)}
          className="whitespace-nowrap text-xs text-text-muted"
        >
          {fmt.formatRelativeTime(row.created_at)}
        </time>
        {link ? (
          <ChevronRight className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" aria-hidden />
        ) : null}
      </>
    );
    const now = new Date();
    return (
      <li
        key={row.id}
        className={cn("space-y-2 p-3", unread && "bg-surface-2/60", nested && "ps-6")}
      >
        {link ? (
          <button
            type="button"
            className="flex w-full items-start gap-2 rounded-md text-start hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            aria-label={t("inbox.openAria", { title: text.title })}
            onClick={() => void openSource(row)}
          >
            {header}
          </button>
        ) : (
          <div className="flex items-start gap-2">{header}</div>
        )}
        <div className="flex flex-wrap gap-1.5 ps-6">
          {needsAck(row) ? (
            <Button
              size="sm"
              variant="outline"
              className="h-8"
              disabled={busy}
              onClick={() => void runAction(row.id, () => acknowledgeNotification(row.id))}
            >
              <Check className="me-1 h-3.5 w-3.5" aria-hidden />
              {t("inbox.actions.acknowledge")}
            </Button>
          ) : null}
          {unread ? (
            <Button
              size="sm"
              variant="ghost"
              className="h-8"
              disabled={busy}
              onClick={() => void runAction(row.id, () => markNotificationRead(row.id))}
            >
              <MailOpen className="me-1 h-3.5 w-3.5" aria-hidden />
              {t("inbox.actions.markRead")}
            </Button>
          ) : row.read_at && !row.acknowledged_at && !row.archived_at && !row.resolved_at ? (
            <Button
              size="sm"
              variant="ghost"
              className="h-8"
              disabled={busy}
              onClick={() => void runAction(row.id, () => markNotificationUnread(row.id))}
            >
              {t("inbox.actions.markUnread")}
            </Button>
          ) : null}
          {!row.acknowledged_at && !row.archived_at && !row.resolved_at ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button size="sm" variant="ghost" className="h-8" disabled={busy}>
                  <Clock className="me-1 h-3.5 w-3.5" aria-hidden />
                  {t("inbox.actions.snooze")}
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem
                  onSelect={() =>
                    void runAction(row.id, () =>
                      snoozeNotification(row.id, new Date(now.getTime() + 3_600_000)),
                    )
                  }
                >
                  {t("inbox.snooze.oneHour")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() =>
                    void runAction(row.id, () =>
                      snoozeNotification(row.id, new Date(now.getTime() + 4 * 3_600_000)),
                    )
                  }
                >
                  {t("inbox.snooze.fourHours")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  onSelect={() =>
                    void runAction(row.id, () =>
                      snoozeNotification(row.id, tomorrowMorning(now, 8)),
                    )
                  }
                >
                  {t("inbox.snooze.tomorrow")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          ) : null}
          {row.archived_at ? (
            <Button
              size="sm"
              variant="ghost"
              className="h-8"
              disabled={busy}
              onClick={() => void runAction(row.id, () => unarchiveNotification(row.id))}
            >
              <ArchiveRestore className="me-1 h-3.5 w-3.5" aria-hidden />
              {t("inbox.actions.unarchive")}
            </Button>
          ) : !needsAck(row) ? (
            <Button
              size="sm"
              variant="ghost"
              className="h-8"
              disabled={busy}
              onClick={() => void runAction(row.id, () => archiveNotification(row.id))}
            >
              <Archive className="me-1 h-3.5 w-3.5" aria-hidden />
              {t("inbox.actions.archive")}
            </Button>
          ) : null}
        </div>
      </li>
    );
  };

  return (
    <div className={cn("flex min-h-0 flex-col", className)}>
      <div aria-live="polite" role="status" className="sr-only">
        {announcement}
      </div>
      {!compact ? (
        <div className="space-y-3 border-b border-surface-border bg-white p-3">
          <Tabs value={filters.view} onValueChange={(v) => setFilter({ view: v as InboxView })}>
            <TabsList
              className="flex w-full flex-wrap justify-start"
              aria-label={t("inbox.tabsAria")}
            >
              {INBOX_VIEWS.map((key) => (
                <TabsTrigger key={key} value={key}>
                  {t(`inbox.tabs.${key}`)}
                  {key === "unread" && counts.unread > 0 ? ` (${counts.unread})` : ""}
                  {key === "to_acknowledge" && counts.to_acknowledge > 0
                    ? ` (${counts.to_acknowledge})`
                    : ""}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          <div className="flex flex-wrap items-end gap-3">
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("filters.module")}
              <Select value={filters.module} onValueChange={(v) => setFilter({ module: v })}>
                <SelectTrigger className="h-9 w-44" aria-label={t("filters.module")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={ALL}>{t("filters.allModules")}</SelectItem>
                  {NOTIFICATION_MODULES.map((m) => (
                    <SelectItem key={m} value={m}>
                      {t(`module.${m}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("filters.severity")}
              <Select value={filters.severity} onValueChange={(v) => setFilter({ severity: v })}>
                <SelectTrigger className="h-9 w-36" aria-label={t("filters.severity")}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={ALL}>{t("filters.allSeverities")}</SelectItem>
                  {NOTIFICATION_SEVERITIES.map((s) => (
                    <SelectItem key={s} value={s}>
                      {t(`severity.${s}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("filters.from")}
              <Input
                type="date"
                className="h-9 w-40"
                value={filters.from}
                max={filters.to || undefined}
                onChange={(e) => setFilter({ from: e.target.value })}
              />
            </label>
            <label className="flex flex-col gap-1 text-xs text-text-muted">
              {t("filters.to")}
              <Input
                type="date"
                className="h-9 w-40"
                value={filters.to}
                min={filters.from || undefined}
                onChange={(e) => setFilter({ to: e.target.value })}
              />
            </label>
            {filtered ? (
              <Button
                variant="ghost"
                size="sm"
                className="h-9"
                onClick={() => setFilters((f) => ({ ...INITIAL, view: f.view }))}
              >
                {t("filters.clear")}
              </Button>
            ) : null}
            <label className="flex items-center gap-2 text-sm text-text-primary">
              <Switch
                checked={grouped}
                onCheckedChange={setGrouped}
                aria-label={t("inbox.groupByObject")}
              />
              {t("inbox.groupByObject")}
            </label>
            <div className="ms-auto">
              <Button
                variant="outline"
                size="sm"
                className="h-9"
                disabled={bulkBusy || counts.unread === 0}
                onClick={() => void markAll()}
              >
                <CheckCheck className="me-1 h-4 w-4" aria-hidden />
                {t("inbox.actions.markAllRead")}
              </Button>
            </div>
          </div>
        </div>
      ) : null}

      <div
        className={cn(
          "min-h-0 overflow-auto bg-white",
          compact ? "max-h-[26rem] rounded-md border border-surface-border" : "flex-1",
        )}
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
            {filtered ? t("inbox.emptyFiltered") : t(`inbox.empty.${filters.view}`)}
          </div>
        ) : null}

        <ul className="divide-y divide-surface-border" aria-label={t("inbox.listAria")}>
          {groups.map((group) => {
            const [first] = group;
            if (!first) return null;
            if (group.length === 1) return renderRow(first, false);
            const code = first.message_params?.["code"];
            return (
              <li key={`g-${first.id}`} className="py-1">
                <div className="flex items-center gap-2 px-3 pt-2 text-xs font-semibold uppercase tracking-wide text-text-muted">
                  {t(`module.${first.module_code}`)}
                  {typeof code === "string" ? ` · ${code}` : ""}
                  <span className="font-normal normal-case">
                    {t("inbox.groupCount", { count: group.length })}
                  </span>
                </div>
                <ul className="divide-y divide-surface-border">
                  {group.map((row) => renderRow(row, true))}
                </ul>
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
        {hasMore && !loading && !error ? (
          <div className="flex justify-center p-2">
            <Button size="sm" variant="ghost" onClick={() => void loadPage(false)}>
              {t("inbox.loadMore")}
            </Button>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function SeverityIcon({ severity }: { severity: string }) {
  const tone = toneOf(NOTIFICATION_SEVERITY_TONE, severity);
  const Icon = severity === "critical" ? Siren : severity === "warning" ? BellRing : Info;
  return <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone[tone].text)} aria-hidden />;
}
