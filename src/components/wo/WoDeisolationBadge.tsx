/**
 * "Déconsignation à faire": the work order is mechanically complete ("Terminé") and a lock is
 * still fitted under one of its permits (owner decision D4). Derived by the backend on read
 * (`WorkOrder.deisolation_pending`); the equipment stays down until the removal of the last lock
 * is verified.
 */
import { Lock } from "lucide-react";
import { useTranslation } from "react-i18next";

import { StatusChip } from "@/components/ui/status-chip";
import type { WorkOrder } from "@shared/ipc-types";

export function WoDeisolationBadge({
  wo,
  className,
}: {
  wo: Pick<WorkOrder, "deisolation_pending">;
  className?: string;
}) {
  const { t } = useTranslation("ot");
  if (!wo.deisolation_pending) return null;
  return (
    <StatusChip
      tone="warning"
      icon={Lock}
      label={t("deisolation.badge")}
      title={t("deisolation.title")}
      {...(className ? { className } : {})}
    />
  );
}
