/**
 * "À revoir" marker of a list value found in stored data that is not (or no longer) a value of
 * the plant's list: it is kept with its exact code until someone reviews it in Données de
 * référence. `hint` says why when the reason is more specific (e.g. a criticality taken from the
 * old list).
 */
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";

export function ReviewMark({ className, hint }: { className?: string; hint?: string }) {
  const { t } = useTranslation("common");
  return (
    <StatusChip
      tone="warning"
      label={t("listValue.review")}
      title={hint ?? t("listValue.reviewHint")}
      {...(className ? { className } : {})}
    />
  );
}
