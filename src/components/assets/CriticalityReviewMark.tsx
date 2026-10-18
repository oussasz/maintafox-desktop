/**
 * "À revoir" next to an asset's criticality when it is only read from the old criticality list
 * (no value of the published list yet): it still counts, but a planner should set it.
 */
import { useTranslation } from "react-i18next";

import { ReviewMark } from "@/components/ui/review-mark";

export function CriticalityReviewMark({ className }: { className?: string }) {
  const { t } = useTranslation("equipment");
  return <ReviewMark hint={t("criticalityReview.hint")} {...(className ? { className } : {})} />;
}
