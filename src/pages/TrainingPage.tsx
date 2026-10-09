import { GraduationCap } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { TrainingQualificationPanel } from "@/components/personnel/TrainingQualificationPanel";

export function TrainingPage() {
  const { t } = useTranslation("shell");

  return (
    <ModulePageShell icon={GraduationCap} title={t("nav.training")}>
      <TrainingQualificationPanel />
    </ModulePageShell>
  );
}
