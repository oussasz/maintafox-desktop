import { useTranslation } from "react-i18next";

export function RoleImportExportPanel() {
  const { t } = useTranslation("admin");
  return (
    <div className="rounded-lg border border-surface-border bg-surface-1 p-6">
      <h2 className="text-lg font-semibold text-text-primary">
        {t("tabs.importExport", "Import / Export")}
      </h2>
      <p className="mt-2 text-sm text-text-secondary">
        {t(
          "placeholders.importExport",
          "Import/export de rôles — panneau en cours d'implémentation.",
        )}
      </p>
    </div>
  );
}
