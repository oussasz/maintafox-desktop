/**
 * The fields an audit event changed, one line each: "Date de clôture : — → 28/10/2026 18:26".
 * Values are shown as recorded (labels, names, codes), dates in the user's format, yes/no
 * translated. Shared by the audit log (Settings › Activity & audit) and the OT "Historique" tab.
 */
import { useTranslation } from "react-i18next";

import { useFormatters } from "@/hooks/use-formatters";

import { type FieldChange, fieldLabel } from "./activity-labels";

const DATE_ONLY = /^\d{4}-\d{2}-\d{2}$/;
const DATE_TIME = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}/;

interface AuditChangeListProps {
  changes: FieldChange[];
  className?: string;
}

export function AuditChangeList({ changes, className }: AuditChangeListProps) {
  const { t } = useTranslation("activity");
  const { formatDate, formatDateTime, formatNumber } = useFormatters();

  if (changes.length === 0) return null;

  const show = (value: unknown): string => {
    if (value == null || value === "") return t("audit.detail.empty");
    if (typeof value === "boolean") return value ? t("audit.detail.yes") : t("audit.detail.no");
    if (typeof value === "number") return formatNumber(value);
    if (typeof value === "string") {
      if (DATE_ONLY.test(value)) return formatDate(value);
      if (DATE_TIME.test(value)) return formatDateTime(value);
      return value;
    }
    return JSON.stringify(value);
  };

  return (
    <div className={className}>
      <p className="text-text-muted">{t("audit.detail.changes")}</p>
      <ul className="mt-0.5 space-y-0.5" data-testid="audit-change-list">
        {changes.map((c, i) => (
          <li key={`${c.field}-${i}`} className="break-words">
            <span className="font-medium text-text-primary">{fieldLabel(t, c.field)}</span>
            {" : "}
            <span className="text-text-secondary">{show(c.old)}</span>
            {" → "}
            <span className="text-text-primary">{show(c.new)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
