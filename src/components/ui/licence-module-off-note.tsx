/**
 * Shown in place of a module's content when the licence does not include that module: names the
 * module and who can add it, instead of the server's refusals of each of its lists.
 */
import { Lock } from "lucide-react";
import { useTranslation } from "react-i18next";

import { moduleCapabilityFromPermission, moduleLabelKey } from "@/lib/module-capability";
import { cn } from "@/lib/utils";

export function LicenceModuleOffNote({
  permission,
  className,
}: {
  /** A right of the module (`trn.view`): the module is read from its prefix. */
  permission: string;
  className?: string;
}) {
  const { t } = useTranslation(["errors", "shell"]);
  const labelKey = moduleLabelKey(moduleCapabilityFromPermission(permission) ?? "");
  const module = labelKey ? t(`shell:${labelKey}` as never) : t("errors:licence.moduleUnknown");
  return (
    <div
      role="note"
      data-testid="licence-module-off"
      className={cn(
        "flex items-start gap-2 rounded-md border border-border bg-surface-2 p-3 text-sm text-text-muted",
        className,
      )}
    >
      <Lock className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      <div className="space-y-0.5">
        <p className="font-medium text-text-primary">{t("errors:licence.moduleOffTitle")}</p>
        <p>{t("errors:licence.moduleOff", { module })}</p>
      </div>
    </div>
  );
}
