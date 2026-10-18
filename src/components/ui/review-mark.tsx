/**
 * "À revoir" marker of a list value found in stored data that is not (or no longer) a value of
 * the plant's list: it is kept with its exact code until someone reviews it in Données de
 * référence.
 */
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";

export function ReviewMark({ className }: { className?: string }) {
  const { t } = useTranslation("common");
  return (
    <StatusChip
      tone="warning"
      label={t("listValue.review")}
      title={t("listValue.reviewHint")}
      {...(className ? { className } : {})}
    />
  );
}
