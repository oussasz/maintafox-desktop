import { Bell } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { getUnreadCount } from "@/services/notification-service";

const POLL_INTERVAL_MS = 30_000;

export function NotificationBell() {
  const [count, setCount] = useState(0);
  const intervalRef = useRef<number | null>(null);

  const poll = useCallback(async () => {
    try {
      const unread = await getUnreadCount();
      setCount(unread);
    } catch {
      setCount(0);
    }
  }, []);

  useEffect(() => {
    void poll();
    intervalRef.current = window.setInterval(() => void poll(), POLL_INTERVAL_MS);
    return () => {
      if (intervalRef.current !== null) {
        window.clearInterval(intervalRef.current);
      }
    };
  }, [poll]);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={`Notifications (${count})`}
          className="relative h-8 w-8"
        >
          <Bell className="h-4 w-4" />
          {count > 0 && (
            <span
              className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-red-600 px-1 text-[10px] font-semibold text-white"
            >
              {count > 99 ? "99+" : count}
            </span>
          )}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={8} className="w-[34rem] p-3">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-sm font-semibold">Notifications</h3>
          <span className="text-xs text-muted-foreground">{count} unread</span>
        </div>
        <NotificationInbox onChanged={() => void poll()} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
