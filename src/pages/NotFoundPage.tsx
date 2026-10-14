/**
 * Fallback screens for navigation:
 *  - NotFoundPage: any address with no screen (catch-all route inside the shell).
 *  - RouteErrorPage: a screen threw while rendering (router `errorElement`).
 * Both keep the user inside Maintafox with a way back, instead of the router's raw error page.
 */
import { AlertTriangle, Compass } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useNavigate, useRouteError } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { toErrorMessage } from "@/utils/errors";

function FallbackFrame({
  icon,
  title,
  body,
  details,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
  details?: string | null;
}) {
  const { t } = useTranslation("shell");
  const navigate = useNavigate();
  return (
    <div className="flex h-full min-h-[60vh] items-center justify-center p-6">
      <div className="flex max-w-lg flex-col items-center gap-3 text-center" role="alert">
        {icon}
        <h1 className="text-xl font-semibold text-text-primary">{title}</h1>
        <p className="text-sm text-text-secondary">{body}</p>
        {details ? (
          <details className="w-full text-xs text-text-muted">
            <summary className="cursor-pointer select-none">
              {t("notFound.technicalDetails")}
            </summary>
            <p className="mt-1 whitespace-pre-wrap break-words text-start">{details}</p>
          </details>
        ) : null}
        <Button className="mt-2 h-11" onClick={() => void navigate("/")}>
          {t("notFound.home")}
        </Button>
      </div>
    </div>
  );
}

export function NotFoundPage() {
  const { t } = useTranslation("shell");
  return (
    <FallbackFrame
      icon={<Compass className="h-8 w-8 text-text-muted" aria-hidden />}
      title={t("notFound.title")}
      body={t("notFound.body")}
    />
  );
}

export function RouteErrorPage() {
  const { t } = useTranslation("shell");
  const error = useRouteError();
  return (
    <FallbackFrame
      icon={<AlertTriangle className="h-8 w-8 text-tone-danger-fg" aria-hidden />}
      title={t("notFound.errorTitle")}
      body={t("notFound.errorBody")}
      details={error == null ? null : toErrorMessage(error)}
    />
  );
}
