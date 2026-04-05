import { Construction } from "lucide-react";
import { useTranslation } from "react-i18next";

interface Props {
  moduleName: string;
  prdSection: string;
  phase: string;
}

export function ModulePlaceholder({ moduleName, prdSection, phase }: Props) {
  const { t } = useTranslation("shell");
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 text-center p-8">
      <Construction className="h-12 w-12 text-text-muted" />
      <p className="text-xl font-semibold text-text-primary">{moduleName}</p>
      <p className="text-sm text-text-secondary max-w-sm">
        {t("placeholder.notYetImplemented", { phase })}
      </p>
      <p className="text-xs text-text-muted">PRD §{prdSection}</p>
    </div>
  );
}
