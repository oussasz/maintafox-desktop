import { Bell } from "lucide-react";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { NotificationPreferencesPanel } from "@/components/notifications/NotificationPreferencesPanel";

export function NotificationsPage() {
  return (
    <ModulePageShell
      icon={Bell}
      title="Notifications"
      description="Inbox, acknowledgements, snoozes, and per-category preferences."
      bodyClassName="space-y-4 p-4"
    >
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,2fr)_minmax(0,1.5fr)]">
        <div className="rounded-lg border border-surface-border p-4">
          <NotificationInbox />
        </div>
        <NotificationPreferencesPanel />
      </div>
    </ModulePageShell>
  );
}
