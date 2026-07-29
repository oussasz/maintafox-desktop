/**
 * Read-only list of rows from `di_state_transition_log` (formal state machine audit).
 */

import { useTranslation } from "react-i18next";

import { diStatusToI18nKey } from "@/components/di/status-meta";
import { formatPersonLabel } from "@/lib/display";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { DiTransitionRow } from "@shared/ipc-types";

interface DiStateTransitionListProps {
  transitions: DiTransitionRow[];
}

export function DiStateTransitionList({ transitions }: DiStateTransitionListProps) {
  const { t, i18n } = useTranslation("di");
  const locale = intlLocaleForLanguage(i18n.language);

  if (transitions.length === 0) {
    return <p className="py-6 text-center text-sm text-muted-foreground">{t("stateLog.empty")}</p>;
  }

  return (
    <div className="overflow-x-auto rounded-md border border-surface-border">
      <table className="w-full text-left text-xs">
        <thead className="bg-muted/50 text-muted-foreground">
          <tr>
            <th className="px-3 py-2 font-medium">{t("stateLog.at")}</th>
            <th className="px-3 py-2 font-medium">{t("stateLog.from")}</th>
            <th className="px-3 py-2 font-medium">{t("stateLog.to")}</th>
            <th className="px-3 py-2 font-medium">{t("stateLog.action")}</th>
            <th className="px-3 py-2 font-medium">{t("stateLog.actor")}</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-surface-border">
          {transitions.map((row) => {
            const eventLabel = t(
              `stateLog.actionLabel.${row.action}` as "stateLog.actionLabel.convert",
              {
                defaultValue: t("stateLog.unknownAction", { action: row.action }),
              },
            );
            return (
              <tr key={row.id} className="hover:bg-muted/30">
                <td className="px-3 py-2 whitespace-nowrap font-mono text-[11px]">
                  {new Date(row.acted_at).toLocaleString(locale)}
                </td>
                <td className="px-3 py-2">
                  {row.from_status === "none"
                    ? t("stateLog.actionLabel.none")
                    : t(`status.${diStatusToI18nKey(row.from_status)}` as "status.new")}
                </td>
                <td className="px-3 py-2">
                  {t(`status.${diStatusToI18nKey(row.to_status)}` as "status.new")}
                </td>
                <td className="px-3 py-2 text-text-primary">{eventLabel}</td>
                <td className="px-3 py-2 text-muted-foreground">
                  {row.actor_id != null
                    ? formatPersonLabel(row.actor_display_name)
                    : t("auditTimeline.actorSystem")}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
