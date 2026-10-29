import { AlertTriangle, WifiOff } from "lucide-react";

import { useMobileT } from "../i18n";
import type { UserError } from "../lib/user-error";

import { Banner, Button } from "./kit";

/** The end of an account lockout in the phone's local time, in the app language. */
function lockoutTime(until: string, language: string): string | null {
  const at = new Date(until);
  if (Number.isNaN(at.getTime())) return null;
  const sameDay = at.toDateString() === new Date().toDateString();
  return new Intl.DateTimeFormat(language, {
    ...(sameDay ? {} : { dateStyle: "medium" }),
    timeStyle: "short",
  }).format(at);
}

/** A failure in the user's words: a title, one sentence, and a retry when the caller offers one. */
export function ErrorBanner({ error, onRetry }: { error: UserError; onRetry?: () => void }) {
  const { t, language } = useMobileT();
  const until = error.until ? lockoutTime(error.until, language) : null;
  const Icon = error.kind === "serverUnreachable" ? WifiOff : AlertTriangle;
  return (
    <>
      <Banner
        tone="danger"
        alert
        icon={<Icon size={22} aria-hidden />}
        title={t(`errors.${error.kind}.title`)}
      >
        {t(`errors.${error.kind}.text`, { person: error.person ?? "" })}
        {until ? ` ${t("errors.lockedUntil", { time: until })}` : null}
      </Banner>
      {onRetry ? (
        <div className="mf-actions">
          <Button variant="tinted" onClick={onRetry}>
            {t("common.retry")}
          </Button>
        </div>
      ) : null}
    </>
  );
}
