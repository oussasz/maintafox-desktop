/**
 * The plant rule that made a work order "permit required" when it was converted from a request
 * (Données de référence › Règles d'exigence de permis), and how that flag was cleared.
 *
 * Clearing it is not an edit of the work order: it needs the permit-issue right (`ptw.issue`) and
 * a reason of at least 10 characters, before execution starts; it is confirmed first and kept on
 * the work order with its date and reason.
 *
 * The same applies to a requirement carried from a request flagged "sécurité"
 * (`permit_from_safety_di`, CMMS Advisor decision D1), with the permit-issue right at the work
 * order's site.
 */
import { ShieldCheck } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { usePlantWideAccess, useWoAccess } from "@/components/wo/WoSiteScope";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { PERMIT_REQUIREMENT_EDITABLE_STATUSES, clearWoPermitRuleFlag } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Shortest reason accepted to clear a rule flag (backend `MIN_CLEAR_REASON_CHARS`). */
export const MIN_PERMIT_RULE_CLEAR_REASON = 10;

/**
 * A protected permit requirement still in force: set by a plant rule or carried from a request
 * flagged "sécurité", not cleared, and the work order needs a permit. The general edit cannot
 * untick it (the backend refuses it too).
 */
export function permitRuleFlagActive(wo: WorkOrder): boolean {
  return (
    wo.requires_permit &&
    wo.permit_rule_cleared_at == null &&
    (wo.permit_rule_code != null || wo.permit_from_safety_di === true)
  );
}

/** The protected requirement comes from a safety request only (no plant rule matched). */
export function permitFromSafetyRequestOnly(wo: WorkOrder): boolean {
  return wo.permit_rule_code == null && wo.permit_from_safety_di === true;
}

export function WoPermitRuleNote({
  wo,
  onCleared,
  className,
}: {
  wo: WorkOrder;
  /** Called with the saved work order once the flag is cleared. */
  onCleared?: (saved: WorkOrder) => void | Promise<void>;
  className?: string;
}) {
  const { t } = useTranslation("ot");
  const { formatDateTime } = useFormatters();
  const reasonId = useId();
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState("");
  // The backend clears a rule flag for a plant-wide permit-issue right only (a site role does not
  // lift a plant rule), and a safety request's requirement for the right at the work order's site:
  // the button follows the same rule.
  const plantWide = usePlantWideAccess();
  const atSite = useWoAccess(wo);

  if (permitFromSafetyRequestOnly(wo)) {
    return (
      <SafetyRequestNote
        wo={wo}
        canClear={atSite.can(P.PTW_ISSUE)}
        onCleared={onCleared}
        className={className}
      />
    );
  }

  if (wo.permit_rule_code == null) return null;
  const { can } = plantWide;

  if (wo.permit_rule_cleared_at != null) {
    return (
      <p className={cn("text-xs text-text-secondary", className)} data-testid="wo-permit-rule">
        {t("permitRule.cleared", {
          rule: wo.permit_rule_code,
          date: formatDateTime(wo.permit_rule_cleared_at),
          reason: wo.permit_rule_clear_reason ?? "",
        })}
      </p>
    );
  }

  const active = permitRuleFlagActive(wo);
  const clearable = active && PERMIT_REQUIREMENT_EDITABLE_STATUSES.has(wo.status_code ?? "");
  const reasonOk = reason.trim().length >= MIN_PERMIT_RULE_CLEAR_REASON;

  return (
    <div
      className={cn("rounded-md border px-3 py-2 text-sm", mfTone.warning.panel, className)}
      data-testid="wo-permit-rule"
    >
      <p className="flex items-start gap-2">
        <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
        <span>
          {wo.permit_rule_permit_type
            ? t("permitRule.requiredBy", {
                rule: wo.permit_rule_code,
                permitType: wo.permit_rule_permit_type,
              })
            : t("permitRule.requiredByNoType", { rule: wo.permit_rule_code })}
        </span>
      </p>
      {active ? (
        <p className="mt-1 pl-6 text-xs text-text-secondary">{t("permitRule.clearHint")}</p>
      ) : null}
      {clearable && can(P.PTW_ISSUE) ? (
        <div className="mt-2 pl-6">
          <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
            {t("permitRule.clear")}
          </Button>
        </div>
      ) : null}
      <ConfirmDialog
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (!next) setReason("");
        }}
        title={t("permitRule.clearTitle")}
        description={t("permitRule.clearBody", { code: wo.code, rule: wo.permit_rule_code })}
        confirmLabel={t("permitRule.clearConfirm")}
        destructive
        confirmDisabled={!reasonOk}
        onConfirm={async () => {
          // A refusal keeps the dialog open; the reason is in the app-wide toast.
          const saved = await clearWoPermitRuleFlag({
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: reason.trim(),
          });
          await onCleared?.(saved);
        }}
      >
        <div className="space-y-1">
          <Label htmlFor={reasonId}>{t("permitRule.reason")}</Label>
          <Textarea
            id={reasonId}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
          />
          <p className="text-xs text-text-muted">
            {t("permitRule.reasonHint", { min: MIN_PERMIT_RULE_CLEAR_REASON })}
          </p>
        </div>
      </ConfirmDialog>
    </div>
  );
}

/** The requirement carried from a request flagged "sécurité", and how it was cleared. */
function SafetyRequestNote({
  wo,
  canClear,
  onCleared,
  className,
}: {
  wo: WorkOrder;
  canClear: boolean;
  onCleared?: ((saved: WorkOrder) => void | Promise<void>) | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation("ot");
  const { formatDateTime } = useFormatters();
  const reasonId = useId();
  const [open, setOpen] = useState(false);
  const [reason, setReason] = useState("");

  if (wo.permit_rule_cleared_at != null) {
    return (
      <p className={cn("text-xs text-text-secondary", className)} data-testid="wo-permit-rule">
        {t("permitSafety.cleared", {
          date: formatDateTime(wo.permit_rule_cleared_at),
          reason: wo.permit_rule_clear_reason ?? "",
        })}
      </p>
    );
  }

  const active = permitRuleFlagActive(wo);
  if (!active) return null;
  const clearable = PERMIT_REQUIREMENT_EDITABLE_STATUSES.has(wo.status_code ?? "");
  const reasonOk = reason.trim().length >= MIN_PERMIT_RULE_CLEAR_REASON;

  return (
    <div
      className={cn("rounded-md border px-3 py-2 text-sm", mfTone.warning.panel, className)}
      data-testid="wo-permit-rule"
    >
      <p className="flex items-start gap-2">
        <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
        <span>{t("permitRequirement.fromSafetyDi")}</span>
      </p>
      <p className="mt-1 pl-6 text-xs text-text-secondary">{t("permitRule.clearHint")}</p>
      {clearable && canClear ? (
        <div className="mt-2 pl-6">
          <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
            {t("permitRule.clear")}
          </Button>
        </div>
      ) : null}
      <ConfirmDialog
        open={open}
        onOpenChange={(next) => {
          setOpen(next);
          if (!next) setReason("");
        }}
        title={t("permitRule.clearTitle")}
        description={t("permitSafety.clearBody", { code: wo.code })}
        confirmLabel={t("permitRule.clearConfirm")}
        destructive
        confirmDisabled={!reasonOk}
        onConfirm={async () => {
          // A refusal keeps the dialog open; the reason is in the app-wide toast.
          const saved = await clearWoPermitRuleFlag({
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: reason.trim(),
          });
          await onCleared?.(saved);
        }}
      >
        <div className="space-y-1">
          <Label htmlFor={reasonId}>{t("permitRule.reason")}</Label>
          <Textarea
            id={reasonId}
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
          />
          <p className="text-xs text-text-muted">
            {t("permitSafety.reasonHint", { min: MIN_PERMIT_RULE_CLEAR_REASON })}
          </p>
        </div>
      </ConfirmDialog>
    </div>
  );
}
