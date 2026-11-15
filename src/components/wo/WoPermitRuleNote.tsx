/**
 * The plant rules that made a work order "permit required", whatever created it (Données de
 * référence › Règles d'exigence de permis; owner decision D2 (a)), and how each requirement was
 * cleared. Every permit type the rules give is required (CMMS Advisor 2026-11-04): the work order
 * lists them (`required_permits`), each live (a permit of that type is linked), missing or cleared;
 * the start and the resume need a live permit of every type not cleared.
 *
 * Clearing one is not an edit of the work order: it needs the permit-issue right (`ptw.issue`) at
 * the work order's site (lead decision D-a) and a reason of at least 10 characters, before
 * execution starts; one type at a time, confirmed first and kept on the work order with its date
 * and reason.
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
import { useWoAccess } from "@/components/wo/WoSiteScope";
import { mfTone } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { cn } from "@/lib/utils";
import { PERMIT_REQUIREMENT_EDITABLE_STATUSES, clearWoPermitRuleFlag } from "@/services/wo-service";
import type { RequiredPermitView, WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Shortest reason accepted to clear a rule flag (backend `MIN_CLEAR_REASON_CHARS`). */
export const MIN_PERMIT_RULE_CLEAR_REASON = 10;

/**
 * A protected permit requirement still in force: set by a plant rule or carried from a request
 * flagged "sécurité", not cleared, and the work order needs a permit; or a permit type the rules
 * require that is not cleared. The general edit cannot untick it (the backend refuses it too).
 */
export function permitRuleFlagActive(wo: WorkOrder): boolean {
  const legacy =
    wo.requires_permit &&
    wo.permit_rule_cleared_at == null &&
    (wo.permit_rule_code != null || wo.permit_from_safety_di === true);
  return legacy || (wo.required_permits ?? []).some((p) => p.state !== "cleared");
}

/** The protected requirement comes from a safety request only (no plant rule matched). */
export function permitFromSafetyRequestOnly(wo: WorkOrder): boolean {
  return (
    wo.permit_rule_code == null &&
    wo.permit_from_safety_di === true &&
    (wo.required_permits ?? []).length === 0
  );
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
  // The backend clears a rule's permit type and a safety request's requirement for the
  // permit-issue right at the work order's site (lead decision D-a): the button follows the same
  // rule.
  const atSite = useWoAccess(wo);

  if ((wo.required_permits ?? []).length > 0) {
    return (
      <RequiredPermitsNote
        wo={wo}
        permits={wo.required_permits ?? []}
        canClear={atSite.can(P.PTW_ISSUE)}
        onCleared={onCleared}
        className={className}
      />
    );
  }

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
  const { can } = atSite;

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

/**
 * Every permit type the plant's rules require of the work order: its name, its rule (the primary
 * one marked), its state — the linked permit when one is live, "missing", or who lifted it, when and
 * why — and, for a type not lifted before execution starts, a "Lever" button (the permit-issue right
 * at the work order's site) that lifts that type only, after a confirmation with a reason.
 */
function RequiredPermitsNote({
  wo,
  permits,
  canClear,
  onCleared,
  className,
}: {
  wo: WorkOrder;
  permits: RequiredPermitView[];
  canClear: boolean;
  onCleared?: ((saved: WorkOrder) => void | Promise<void>) | undefined;
  className?: string | undefined;
}) {
  const { t } = useTranslation("ot");
  const { formatDateTime } = useFormatters();
  const reasonId = useId();
  const [lifting, setLifting] = useState<RequiredPermitView | null>(null);
  const [reason, setReason] = useState("");

  const editable = PERMIT_REQUIREMENT_EDITABLE_STATUSES.has(wo.status_code ?? "");
  const anyRequired = permits.some((p) => p.state !== "cleared");
  const reasonOk = reason.trim().length >= MIN_PERMIT_RULE_CLEAR_REASON;

  return (
    <div
      className={cn(
        "rounded-md border px-3 py-2 text-sm",
        anyRequired ? mfTone.warning.panel : "border-surface-border",
        className,
      )}
      data-testid="wo-permit-rule"
    >
      <p className="flex items-start gap-2">
        <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
        <span>{t("permitRule.listTitle")}</span>
      </p>
      <ul className="mt-1 space-y-1 pl-6">
        {permits.map((p) => (
          <li
            key={p.permit_type_code}
            className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs"
            data-testid={`wo-required-permit-${p.permit_type_code}`}
            data-state={p.state}
          >
            <span className="font-medium">{p.permit_type_name}</span>
            <span className="text-text-secondary">
              {t("permitRule.listRule", { rule: p.rule_code })}
            </span>
            {p.primary ? (
              <span className="rounded border border-surface-border px-1 text-[11px] text-text-secondary">
                {t("permitRule.listPrimary")}
              </span>
            ) : null}
            <span
              className={cn(
                p.state === "missing" && cn("font-medium", mfTone.danger.text),
                p.state !== "missing" && "text-text-secondary",
              )}
            >
              {p.state === "live"
                ? t("permitRule.listLive", {
                    permit: p.live_permit_code ?? "",
                    status: p.live_permit_status ?? "",
                  })
                : p.state === "missing"
                  ? t("permitRule.listMissing")
                  : p.cleared_by_name
                    ? t("permitRule.listCleared", {
                        date: p.cleared_at ? formatDateTime(p.cleared_at) : "",
                        by: p.cleared_by_name,
                        reason: p.clear_reason ?? "",
                      })
                    : t("permitRule.listClearedNoName", {
                        date: p.cleared_at ? formatDateTime(p.cleared_at) : "",
                        reason: p.clear_reason ?? "",
                      })}
            </span>
            {p.state !== "cleared" && editable && canClear ? (
              <Button
                variant="outline"
                size="sm"
                className="h-6 px-2 text-xs"
                onClick={() => setLifting(p)}
                aria-label={`${t("permitRule.listClear")} ${p.permit_type_name}`}
              >
                {t("permitRule.listClear")}
              </Button>
            ) : null}
          </li>
        ))}
      </ul>
      {anyRequired ? (
        <p className="mt-1 pl-6 text-xs text-text-secondary">{t("permitRule.clearHint")}</p>
      ) : null}
      <ConfirmDialog
        open={lifting != null}
        onOpenChange={(next) => {
          if (!next) {
            setLifting(null);
            setReason("");
          }
        }}
        title={t("permitRule.clearTypeTitle", { permitType: lifting?.permit_type_name ?? "" })}
        description={t("permitRule.clearTypeBody", {
          code: wo.code,
          permitType: lifting?.permit_type_name ?? "",
          rule: lifting?.rule_code ?? "",
        })}
        confirmLabel={t("permitRule.clearConfirm")}
        destructive
        confirmDisabled={!reasonOk}
        onConfirm={async () => {
          if (lifting == null) return;
          // A refusal keeps the dialog open; the reason is in the app-wide toast.
          const saved = await clearWoPermitRuleFlag({
            wo_id: wo.id,
            expected_row_version: wo.row_version,
            reason: reason.trim(),
            permit_type_code: lifting.permit_type_code,
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
