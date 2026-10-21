/**
 * DiNextStepLabel.tsx — "what happens next" for a request, in plain words.
 * Orange (attention) only when the requester has to act: answer the reviewer's question.
 */

import { useTranslation } from "react-i18next";

import { diNextStep } from "@/components/di/di-next-step";
import { dispositionLabelKey } from "@/components/di/disposition-meta";
import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import { formatDate, intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

export function DiNextStepLabel({
  di,
  className,
}: {
  di: InterventionRequest;
  className?: string;
}) {
  const { t, i18n } = useTranslation("di");
  const step = diNextStep(di);
  const locale = intlLocaleForLanguage(i18n.language);
  const dispositionKey = dispositionLabelKey(di.disposition_code);
  const text = t(step.key, {
    date: formatDate(step.until, locale, { day: "2-digit", month: "2-digit", year: "numeric" }),
    wo: step.woCode ?? "—",
    disposition: dispositionKey ? t(dispositionKey as "disposition.other") : t("status.closed"),
  });
  return (
    <span
      className={cn(
        "text-xs",
        step.requesterAction ? cn("font-medium", mfTone.attention.text) : "text-text-secondary",
        className,
      )}
    >
      {text}
    </span>
  );
}
