import { Bell } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useOsToasts } from "@/hooks/use-os-toasts";
import { useSessionStore } from "@/store/session-store";
import { useNotificationStore } from "@/stores/notification-store";

/** How often the bell asks for the counts (the backend sweep runs every minute). */
export const NOTIFICATION_POLL_MS = 30_000;

export function NotificationBell() {
  const [open, setOpen] = useState(false);
  const authenticated = useSessionStore((s) => s.info?.is_authenticated === true);
  const locked = useSessionStore((s) => s.info?.is_locked === true);
  const counts = useNotificationStore((s) => s.counts);
  const refresh = useNotificationStore((s) => s.refresh);
  const reset = useNotificationStore((s) => s.reset);
  const { t } = useTranslation("notifications");
  useOsToasts(authenticated && !locked);

  useEffect(() => {
    if (!authenticated || locked) {
      reset();
      return;
    }
    void refresh();
    const id = window.setInterval(() => void refresh(), NOTIFICATION_POLL_MS);
    return () => window.clearInterval(id);
  }, [authenticated, locked, refresh, reset]);

  const count = counts.unread;
  return (
    <DropdownMenu open={open} onOpenChange={setOpen}>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={t("bell.aria", { count })}
          className="relative h-8 w-8"
        >
          <Bell className="h-4 w-4" aria-hidden />
          {count > 0 && (
            <span className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-accent px-1 text-[10px] font-semibold text-brand-navy">
              {count > 99 ? "99+" : count}
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={8} className="w-[34rem] p-3">
        <div className="mb-2 flex items-center justify-between gap-2">
          <h3 className="text-sm font-semibold">{t("bell.title")}</h3>
          <span className="text-xs text-text-muted">
            {t("bell.unread", { count })}
            {counts.to_acknowledge > 0
              ? ` · ${t("bell.toAcknowledge", { count: counts.to_acknowledge })}`
              : ""}
          </span>
        </div>
        {open ? <NotificationInbox variant="compact" onNavigate={() => setOpen(false)} /> : null}
        <div className="mt-2 flex justify-end">
          <Button asChild variant="link" size="sm" className="h-8 px-0">
            <Link to="/notifications" onClick={() => setOpen(false)}>
              {t("bell.openInbox")}
            </Link>
          </Button>
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
