import { HardHat } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Button } from "@/components/ui";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import {
  TERMINAL_PERMIT_STATUSES,
  type WorkPermitRecord,
  getPermitForWorkOrder,
} from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

import { PERMIT_QUERY_CREATE_FOR_WO, PERMIT_QUERY_SELECT } from "./permit-rules";

interface WoPermitGateCardProps {
  workOrderId: number;
  /** Bump to refetch (e.g. after the WO row version changes). */
  refreshKey?: number | string;
}

/**
 * Shown on a WO that `requires_permit`: tells the user whether the PTW gate
 * (`permit/wo_gate.rs`) will let the WO start, and links to create / open the permit.
 */
export function WoPermitGateCard({ workOrderId, refreshKey }: WoPermitGateCardProps) {
  const { t } = useTranslation("permits");
  const { can } = usePermissions();
  const navigate = useNavigate();
  const [permit, setPermit] = useState<WorkPermitRecord | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    let cancelled = false;
    setLoaded(false);
    setLoadError(null);
    getPermitForWorkOrder(workOrderId)
      .then((p) => {
        if (!cancelled) setPermit(p);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setPermit(null);
          setLoadError(describeAppError(err).title);
        }
      })
      .finally(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, [workOrderId, refreshKey, reload]);

  if (!can(P.PTW_VIEW) || !loaded) return null;

  if (loadError) {
    return (
      <section
        aria-label={t("woGate.title")}
        data-testid="wo-permit-gate"
        role="alert"
        className="flex flex-wrap items-center gap-3 rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm"
      >
        <HardHat className="h-4 w-4 shrink-0 text-tone-danger-fg" aria-hidden />
        <p className="min-w-0 flex-1 text-text-primary">
          {t("woGate.loadError", { message: loadError })}
        </p>
        <Button size="sm" variant="outline" onClick={() => setReload((n) => n + 1)}>
          {t("actions.retry")}
        </Button>
      </section>
    );
  }

  const live = permit != null && !TERMINAL_PERMIT_STATUSES.has(permit.status) ? permit : null;
  const ready = live?.status === "active";

  return (
    <section
      aria-label={t("woGate.title")}
      data-testid="wo-permit-gate"
      className={`flex flex-wrap items-center gap-3 rounded-md border px-3 py-2 text-sm ${
        ready
          ? "border-tone-success-border bg-tone-success-bg"
          : "border-tone-warning-border bg-tone-warning-bg"
      }`}
    >
      <HardHat className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="font-medium text-text-primary">
          {ready ? t("woGate.ready") : t("woGate.required")}
        </p>
        <p className="text-xs text-text-muted">
          {live
            ? t("woGate.linked", { code: live.code, status: t(`status.${live.status}`) })
            : t("woGate.none")}
        </p>
      </div>
      {live ? (
        <Button
          size="sm"
          variant="outline"
          onClick={() => navigate(`/permits?${PERMIT_QUERY_SELECT}=${live.id}`)}
        >
          {t("woGate.open")}
        </Button>
      ) : can(P.PTW_ISSUE) ? (
        <Button
          size="sm"
          onClick={() => navigate(`/permits?${PERMIT_QUERY_CREATE_FOR_WO}=${workOrderId}`)}
        >
          {t("woGate.create")}
        </Button>
      ) : null}
    </section>
  );
}
