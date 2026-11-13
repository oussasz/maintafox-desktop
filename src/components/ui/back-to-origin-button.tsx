import { ArrowLeft } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

/**
 * "← Retour" shown on a detail screen that was opened from another screen (a study, a request,
 * the home page, a notification). It closes the detail and goes back to where the user came from.
 */
export function BackToOriginButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation("common");
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className="-ml-2 mb-1 w-fit gap-1.5 self-start"
      onClick={onClick}
    >
      <ArrowLeft className="h-4 w-4" aria-hidden />
      {t("action.back")}
    </Button>
  );
}

/** react-router numbers its history entries; above 0 the user has a screen to return to. */
export function hasScreenToReturnTo(): boolean {
  const idx = (window.history.state as { idx?: number } | null)?.idx ?? 0;
  return idx > 0;
}
