/**
 * "To watch" block of the bell: late work orders and overdue requests, each one a row that says
 * what is wrong in plain words and opens the list behind the figure. Nothing is shown for a
 * figure that is zero or that the user has no right to see (`use-bell-alerts`).
 */

import { ChevronRight, ClipboardList, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { mfTone } from "@/design-system/tone";
import { listLinks } from "@/lib/list-links";
import { cn } from "@/lib/utils";

interface BellBusinessAlertsProps {
  lateWorkOrders: number | null;
  overdueRequests: number | null;
  onNavigate?: () => void;
}

export function BellBusinessAlerts({
  lateWorkOrders,
  overdueRequests,
  onNavigate,
}: BellBusinessAlertsProps) {
  const { t } = useTranslation("notifications");
  const navigate = useNavigate();
  const rows = [
    lateWorkOrders && lateWorkOrders > 0
      ? {
          key: "wo",
          Icon: Wrench,
          title: t("bell.alerts.lateWorkOrders.title", { count: lateWorkOrders }),
          body: t("bell.alerts.lateWorkOrders.body"),
          action: t("bell.alerts.lateWorkOrders.action"),
          to: listLinks.workOrders({ scope: "overdue" }),
        }
      : null,
    overdueRequests && overdueRequests > 0
      ? {
          key: "di",
          Icon: ClipboardList,
          title: t("bell.alerts.overdueRequests.title", { count: overdueRequests }),
          body: t("bell.alerts.overdueRequests.body"),
          action: t("bell.alerts.overdueRequests.action"),
          to: listLinks.requests(),
        }
      : null,
  ].filter((r) => r !== null);
  if (rows.length === 0) return null;

  return (
    <section className="mb-2" aria-label={t("bell.alerts.aria")}>
      <h4 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-muted">
        {t("bell.alerts.heading")}
      </h4>
      <ul className="divide-y divide-surface-border rounded-md border border-surface-border bg-surface-1">
        {rows.map(({ key, Icon, title, body, action, to }) => (
          <li key={key}>
            <button
              type="button"
              data-testid={`bell-alert-${key}`}
              className="flex w-full items-start gap-2 rounded-md p-3 text-start hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() => {
                navigate(to);
                onNavigate?.();
              }}
            >
              <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone.warning.text)} aria-hidden />
              <span className="min-w-0 flex-1">
                <span className="block text-sm font-semibold text-text-primary">{title}</span>
                <span className="block text-sm text-text-muted">{body}</span>
                <span className="mt-1 block text-xs font-medium text-primary">{action}</span>
              </span>
              <ChevronRight className="mt-0.5 h-4 w-4 shrink-0 text-text-muted" aria-hidden />
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
