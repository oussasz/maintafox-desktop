import { Lock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

/**
 * 403 Unauthorized page. Shown by `<PermissionRoute>` when the user
 * lacks the required permissions for a route.
 */
export function UnauthorizedPage() {
  const { t } = useTranslation("auth");

  return (
    <div className="flex h-full items-center justify-center px-4">
      <div className="text-center max-w-md">
        <div
          className="mx-auto mb-6 flex h-16 w-16 items-center justify-center
                     rounded-full bg-status-danger/10"
        >
          <Lock className="h-8 w-8 text-status-danger" />
        </div>

        <h1 className="text-xl font-semibold text-text-primary">
          {t("unauthorized.title", "Accès non autorisé")}
        </h1>

        <p className="mt-2 text-sm text-text-secondary">
          {t(
            "unauthorized.message",
            "Vous n'avez pas les permissions nécessaires pour accéder à cette page.",
          )}
        </p>

        <Link
          to="/"
          className="mt-6 inline-flex items-center gap-2 rounded-md bg-primary
                     px-4 py-2 text-sm font-medium text-white
                     hover:bg-primary/90 transition-colors"
        >
          {t("unauthorized.backToDashboard", "Retour au tableau de bord")}
        </Link>
      </div>
    </div>
  );
}
