import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { NotificationPreferencesPanel } from "@/components/notifications/NotificationPreferencesPanel";

/**
 * Notifications inbox + preferences — embeddable in Settings (no page shell).
 */
export function NotificationsSettingsPanel() {
  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,2fr)_minmax(0,1.5fr)]">
      <div className="rounded-lg border border-surface-border p-4">
        <NotificationInbox />
      </div>
      <NotificationPreferencesPanel />
    </div>
  );
}
