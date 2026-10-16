import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { NotificationPreferencesPanel } from "@/components/notifications/NotificationPreferencesPanel";

/**
 * Settings → Notifications: the person's own preferences. The inbox is its own page
 * (`/notifications`); who receives what and the escalation delays are the plant's settings
 * (Données de référence → Notifications).
 */
export function NotificationsSettingsPanel() {
  const { t } = useTranslation("notifications");
  return (
    <div className="space-y-3">
      <p className="text-sm text-text-muted">
        {t("preferences.intro")}{" "}
        <Link
          className="font-medium text-primary underline-offset-2 hover:underline"
          to="/notifications"
        >
          {t("preferences.openInbox")}
        </Link>
      </p>
      <NotificationPreferencesPanel />
    </div>
  );
}
