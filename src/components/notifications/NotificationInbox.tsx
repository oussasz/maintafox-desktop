import {
  AlertTriangle,
  BellRing,
  Info,
  Siren,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { toErrorMessage } from "@/utils/errors";
import {
  acknowledgeNotification,
  listNotifications,
  markNotificationRead,
  snoozeNotification,
  type NotificationSummary,
} from "@/services/notification-service";

type InboxTab = "all" | "unread" | "escalated" | "snoozed";

interface NotificationInboxProps {
  className?: string;
  onChanged?: () => void;
}

const PAGE_SIZE = 20;

export function NotificationInbox({ className, onChanged }: NotificationInboxProps) {
  const navigate = useNavigate();
  const [tab, setTab] = useState<InboxTab>("all");
  const [rows, setRows] = useState<NotificationSummary[]>([]);
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadPage = useCallback(
    async (reset: boolean) => {
      if (loading) return;
      setLoading(true);
      setError(null);
      try {
        const nextOffset = reset ? 0 : offset;
        const filter =
          tab === "all"
            ? { limit: PAGE_SIZE, offset: nextOffset }
            : {
                delivery_state: tab === "unread" ? "unread" : tab,
                limit: PAGE_SIZE,
                offset: nextOffset,
              };
        const data = await listNotifications(filter);

        setRows((prev) => (reset ? data : [...prev, ...data]));
        setOffset(nextOffset + data.length);
        setHasMore(data.length === PAGE_SIZE);
      } catch (err) {
        setError(toErrorMessage(err));
      } finally {
        setLoading(false);
      }
    },
    [loading, offset, tab],
  );

  useEffect(() => {
    setOffset(0);
    setHasMore(true);
    void loadPage(true);
  }, [tab]); // eslint-disable-line react-hooks/exhaustive-deps

  const onScroll = useCallback(
    (event: React.UIEvent<HTMLDivElement>) => {
      if (!hasMore || loading) return;
      const el = event.currentTarget;
      const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
      if (nearBottom) {
        void loadPage(false);
      }
    },
    [hasMore, loadPage, loading],
  );

  const handleRead = useCallback(
    async (id: number) => {
      await markNotificationRead(id);
      await loadPage(true);
      onChanged?.();
    },
    [loadPage, onChanged],
  );

  const handleAcknowledge = useCallback(
    async (id: number) => {
      await acknowledgeNotification(id);
      await loadPage(true);
      onChanged?.();
    },
    [loadPage, onChanged],
  );

  const handleSnooze = useCallback(
    async (id: number, minutes: number) => {
      await snoozeNotification(id, minutes);
      await loadPage(true);
      onChanged?.();
    },
    [loadPage, onChanged],
  );

  const emptyLabel = useMemo(() => {
    if (tab === "unread") return "No unread notifications.";
    if (tab === "escalated") return "No escalated notifications.";
    if (tab === "snoozed") return "No snoozed notifications.";
    return "No notifications yet.";
  }, [tab]);

  return (
    <div className={className}>
      <Tabs value={tab} onValueChange={(v) => setTab(v as InboxTab)}>
        <TabsList className="grid w-full grid-cols-4">
          <TabsTrigger value="all">All</TabsTrigger>
          <TabsTrigger value="unread">Unread</TabsTrigger>
          <TabsTrigger value="escalated">Escalated</TabsTrigger>
          <TabsTrigger value="snoozed">Snoozed</TabsTrigger>
        </TabsList>
      </Tabs>

      <div className="mt-3 max-h-[28rem] overflow-auto rounded-md border" onScroll={onScroll}>
        {rows.length === 0 && !loading && !error && (
          <div className="flex h-36 items-center justify-center text-sm text-muted-foreground">
            {emptyLabel}
          </div>
        )}

        {error && (
          <div className="p-3 text-sm text-destructive">
            Failed to load notifications: {error}
          </div>
        )}

        <div className="divide-y">
          {rows.map((row) => (
            <div key={row.id} className="space-y-2 p-3">
              <div className="flex items-start gap-2">
                <SeverityIcon severity={row.severity} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="truncate font-medium">{row.title}</span>
                    <Badge variant="outline">{row.category_code}</Badge>
                    <StateBadge state={row.delivery_state} />
                  </div>
                  {row.body && (
                    <p className="mt-1 line-clamp-2 text-sm text-muted-foreground">{row.body}</p>
                  )}
                </div>
                <span className="whitespace-nowrap text-xs text-muted-foreground">
                  {timeAgo(row.created_at)}
                </span>
              </div>

              <div className="flex flex-wrap gap-2">
                {row.delivery_state === "delivered" && (
                  <Button size="sm" variant="outline" onClick={() => void handleRead(row.id)}>
                    Read
                  </Button>
                )}
                {row.requires_ack && row.delivery_state !== "acknowledged" && (
                  <Button size="sm" variant="outline" onClick={() => void handleAcknowledge(row.id)}>
                    Acknowledge
                  </Button>
                )}
                <Button size="sm" variant="outline" onClick={() => void handleSnooze(row.id, 60)}>
                  Snooze 1h
                </Button>
                <Button size="sm" variant="outline" onClick={() => void handleSnooze(row.id, 240)}>
                  Snooze 4h
                </Button>
                {row.action_url && (
                  <Button
                    size="sm"
                    onClick={() => {
                      if (row.action_url?.startsWith("/")) {
                        navigate(row.action_url);
                      } else if (row.action_url) {
                        window.open(row.action_url, "_blank", "noopener,noreferrer");
                      }
                    }}
                  >
                    Open source record
                  </Button>
                )}
              </div>
            </div>
          ))}
        </div>

        {loading && (
          <div className="flex items-center justify-center py-3 text-sm text-muted-foreground">
            Loading...
          </div>
        )}
      </div>
    </div>
  );
}

function SeverityIcon({ severity }: { severity: string }) {
  if (severity === "critical") return <Siren className="mt-0.5 h-4 w-4 text-destructive" />;
  if (severity === "error") return <AlertTriangle className="mt-0.5 h-4 w-4 text-destructive" />;
  if (severity === "warning") return <BellRing className="mt-0.5 h-4 w-4 text-amber-500" />;
  return <Info className="mt-0.5 h-4 w-4 text-blue-500" />;
}

function StateBadge({ state }: { state: string }) {
  if (state === "escalated") return <Badge className="bg-orange-600 text-white">{state}</Badge>;
  if (state === "acknowledged") return <Badge className="bg-emerald-600 text-white">{state}</Badge>;
  if (state === "snoozed") return <Badge variant="secondary">{state}</Badge>;
  return <Badge variant="outline">{state}</Badge>;
}

function timeAgo(value: string): string {
  const time = new Date(value).getTime();
  if (!Number.isFinite(time)) return value;
  const diff = Date.now() - time;
  if (diff < 60_000) return "just now";
  const minutes = Math.floor(diff / 60_000);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}
