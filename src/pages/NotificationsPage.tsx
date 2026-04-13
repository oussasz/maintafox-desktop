import { Bell } from "lucide-react";

import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { NotificationPreferencesPanel } from "@/components/notifications/NotificationPreferencesPanel";

export function NotificationsPage() {
  return (
    <div className="flex h-full flex-col gap-4 p-6">
      <div className="flex items-center gap-3">
        <Bell className="h-5 w-5 text-primary" />
        <div>
          <h1 className="text-2xl font-semibold">Notifications</h1>
          <p className="text-sm text-muted-foreground">
            Inbox, acknowledgements, snoozes, and per-category preferences.
          </p>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,2fr)_minmax(0,1.5fr)]">
        <div className="rounded-lg border p-4">
          <NotificationInbox />
        </div>
        <NotificationPreferencesPanel />
      </div>
    </div>
  );
}
