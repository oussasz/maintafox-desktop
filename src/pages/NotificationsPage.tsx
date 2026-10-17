import { Bell, Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { NotificationInbox } from "@/components/notifications/NotificationInbox";
import { Button } from "@/components/ui/button";
import { useNotificationStore } from "@/stores/notification-store";

/** The person's notification inbox (preferences live in Settings → Notifications). */
export function NotificationsPage() {
  const { t } = useTranslation("notifications");
  const unread = useNotificationStore((s) => s.counts.unread);
  return (
    <ModulePageShell
      icon={Bell}
      title={t("page.title")}
      count={unread}
      description={t("page.description")}
      actions={
        <Button asChild variant="outline" size="sm">
          <Link to="/settings?tab=notifications">
            <Settings2 className="me-1 h-4 w-4" aria-hidden />
            {t("page.preferences")}
          </Link>
        </Button>
      }
      bodyClassName={null}
    >
      <NotificationInbox variant="page" className="h-full" />
    </ModulePageShell>
  );
}
