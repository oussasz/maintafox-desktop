import { ShieldCheck } from "lucide-react";
import { useTranslation } from "react-i18next";

import { DiagnosticsPanel } from "@/components/diagnostics/DiagnosticsPanel";
import { ModulePageShell } from "@/components/layout/ModulePageShell";

/** Standalone diagnostics route — thin wrapper; prefer Settings → System & Sync. */
export function DiagnosticsPage() {
  const { t } = useTranslation("shell");

  return (
    <ModulePageShell
      icon={ShieldCheck}
      title={t("diagnostics.title")}
      description={t("diagnostics.subtitle")}
      bodyClassName="mx-auto max-w-3xl space-y-6 p-4"
    >
      <DiagnosticsPanel />
    </ModulePageShell>
  );
}
