/**
 * Shows whether a person may be assigned to a job (work order or LOTO role): what they lack and
 * why, expiry warnings, an override already granted — and, for authorised supervisors, the
 * audited override (reason + step-up). Drop-in for the OT labour / assignment dialogs and the
 * permit isolation steps.
 */
import type { TFunction } from "i18next";
import { ShieldAlert, ShieldCheck } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { Textarea } from "@/components/ui/textarea";
import { mfTone } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { cn } from "@/lib/utils";
import {
  type PersonQualificationCheck,
  type QualificationGap,
  type QualificationOverrideContext,
  recordQualificationOverride,
} from "@/services/qualification-gate-service";
import { P } from "@shared/rbac/permissions.generated";

import { GAP_STATUS_TONE } from "./qualification-tones";

/** Shortest override reason the backend accepts. */
export const MIN_OVERRIDE_REASON_CHARS = 10;

/** One translated sentence for a gap ("expired on 2026-09-30", "level 2 held, 3 required"…). */
export function describeGap(t: TFunction<"training">, gap: QualificationGap): string {
  return t(`gap.${gap.status}`, {
    qualification: gap.label,
    expiresAt: gap.expires_at?.slice(0, 10) ?? "",
    held: gap.held_level ?? "",
    required: gap.required_level ?? "",
  });
}

export interface QualificationCheckNoticeProps {
  check: PersonQualificationCheck;
  /** Where an override would apply; omit to hide the override action. */
  overrideContext?: QualificationOverrideContext;
  /** Called with the person's check after an override was recorded. */
  onOverridden?: (check: PersonQualificationCheck) => void;
  className?: string;
}

export function QualificationCheckNotice({
  check,
  overrideContext,
  onOverridden,
  className,
}: QualificationCheckNoticeProps) {
  const { t } = useTranslation("training");
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [askReason, setAskReason] = useState(false);
  const [reason, setReason] = useState("");

  const blocked = !check.qualified;
  const overridden = check.qualified && check.override_info != null;
  const tone = blocked ? "danger" : overridden ? "attention" : "success";
  const canOverride = blocked && overrideContext != null && can(P.TRN_CERTIFY);

  const submitOverride = async () => {
    if (!overrideContext) return;
    const after = await withStepUp(() =>
      recordQualificationOverride(check.personnel_id, overrideContext, reason.trim()),
    );
    setAskReason(false);
    setReason("");
    onOverridden?.(after);
  };

  return (
    <div
      role={blocked ? "alert" : "status"}
      className={cn("rounded-md border p-3 text-sm", mfTone[tone].panel, className)}
    >
      {StepUpDialogElement}
      <div className="flex items-start gap-2">
        {blocked ? (
          <ShieldAlert className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone.danger.text)} aria-hidden />
        ) : (
          <ShieldCheck className={cn("mt-0.5 h-4 w-4 shrink-0", mfTone[tone].text)} aria-hidden />
        )}
        <div className="min-w-0 flex-1 space-y-1.5">
          <p className="font-medium text-text-primary">
            {blocked
              ? t("check.blocked", { person: check.full_name })
              : overridden
                ? t("check.overridden", { person: check.full_name })
                : t("check.qualified", { person: check.full_name })}
          </p>
          {check.gaps.length > 0 ? (
            <ul className="space-y-1">
              {check.gaps.map((g) => (
                <li key={g.requirement_key} className="flex flex-wrap items-center gap-2">
                  <StatusChip
                    label={t(`gapStatus.${g.status}`)}
                    tone={GAP_STATUS_TONE[g.status]}
                    dot
                  />
                  <span>{describeGap(t, g)}</span>
                </li>
              ))}
            </ul>
          ) : null}
          {check.warnings.length > 0 ? (
            <ul className="space-y-1">
              {check.warnings.map((w) => (
                <li key={w.requirement_key} className={mfTone.attention.text}>
                  {t("check.expiresSoon", {
                    qualification: w.label,
                    expiresAt: w.expires_at.slice(0, 10),
                  })}
                </li>
              ))}
            </ul>
          ) : null}
          {overridden && check.override_info ? (
            <p className="text-text-muted">
              {t("check.overrideBy", {
                name: check.override_info.approved_by_name ?? "—",
                date: check.override_info.approved_at.slice(0, 10),
                reason: check.override_info.reason,
              })}
            </p>
          ) : null}
          {blocked ? (
            <p className="text-text-muted">
              {canOverride ? t("check.howToProceedSupervisor") : t("check.howToProceed")}
            </p>
          ) : null}
        </div>
        {canOverride ? (
          <Button size="sm" variant="outline" onClick={() => setAskReason(true)}>
            {t("check.override")}
          </Button>
        ) : null}
      </div>

      <ConfirmDialog
        open={askReason}
        onOpenChange={(o) => {
          setAskReason(o);
          if (!o) setReason("");
        }}
        title={t("check.overrideTitle", { person: check.full_name })}
        description={t("check.overrideDescription")}
        confirmLabel={t("check.overrideConfirm")}
        confirmDisabled={reason.trim().length < MIN_OVERRIDE_REASON_CHARS}
        onConfirm={submitOverride}
      >
        <label className="block space-y-1 text-sm">
          <span className="font-medium">{t("check.overrideReason")}</span>
          <Textarea
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            rows={3}
            aria-describedby="override-reason-hint"
          />
          <span id="override-reason-hint" className="text-xs text-text-muted">
            {t("check.overrideReasonHint", { min: MIN_OVERRIDE_REASON_CHARS })}
          </span>
        </label>
      </ConfirmDialog>
    </div>
  );
}
