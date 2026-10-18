/**
 * "Needs shutdown" for a work order being prepared (draft, planning, ready). When set, planning
 * checks the job against shutdown windows like a PM plan (the warnings come from the backend).
 * Each change is confirmed, then saved at once with the version the planner saw; outside the
 * preparation statuses the box is disabled and says why.
 */
import { Power } from "lucide-react";
import { useId, useState } from "react";
import { useTranslation } from "react-i18next";

import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { StatusChip } from "@/components/ui/status-chip";
import { mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";

export function WoShutdownRequirementField({
  woCode,
  checked,
  onConfirmChange,
  disabled = false,
  disabledReason = null,
  className,
}: {
  woCode: string;
  checked: boolean;
  /** Saves the new value; rejects when the save is refused (the dialog then stays open). */
  onConfirmChange: (next: boolean) => Promise<void>;
  disabled?: boolean;
  /** Visible explanation when the control is disabled. */
  disabledReason?: string | null;
  className?: string;
}) {
  const { t } = useTranslation("ot");
  const id = useId();
  const helpId = `${id}-help`;
  const [pending, setPending] = useState<boolean | null>(null);

  return (
    <div
      className={cn(
        "rounded-md border px-3 py-2",
        checked ? mfTone.warning.panel : "border-surface-border",
        className,
      )}
      data-testid="wo-shutdown-requirement"
    >
      <label
        htmlFor={id}
        className={cn(
          "flex min-h-[44px] items-center gap-3",
          disabled ? "cursor-not-allowed" : "cursor-pointer",
        )}
      >
        <input
          id={id}
          type="checkbox"
          className="h-5 w-5 shrink-0 rounded border border-input accent-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
          checked={checked}
          disabled={disabled}
          aria-describedby={helpId}
          onChange={(e) => setPending(e.target.checked)}
        />
        <Power className="h-4 w-4 shrink-0 text-text-muted" aria-hidden />
        <span className="text-sm font-medium text-text-primary">
          {t("shutdownRequirement.label")}
        </span>
      </label>
      <div id={helpId} className="space-y-1 pl-8 text-xs text-text-muted">
        <p>{t("shutdownRequirement.help")}</p>
        {disabled && disabledReason ? <p>{disabledReason}</p> : null}
      </div>
      <ConfirmDialog
        open={pending !== null}
        onOpenChange={(open) => {
          if (!open) setPending(null);
        }}
        title={
          pending
            ? t("shutdownRequirement.confirmOnTitle")
            : t("shutdownRequirement.confirmOffTitle")
        }
        description={
          pending
            ? t("shutdownRequirement.confirmOnBody", { code: woCode })
            : t("shutdownRequirement.confirmOffBody", { code: woCode })
        }
        confirmLabel={
          pending ? t("shutdownRequirement.confirmOn") : t("shutdownRequirement.confirmOff")
        }
        onConfirm={async () => {
          const next = pending;
          if (next === null) return;
          // A refusal keeps the dialog open (the reason is in the app-wide toast).
          await onConfirmChange(next);
        }}
      />
    </div>
  );
}

/** Badge of a work order that needs the equipment stopped (lists, board, detail). */
export function WoShutdownBadge({ className }: { className?: string }) {
  const { t } = useTranslation("ot");
  return (
    <StatusChip
      tone="warning"
      icon={Power}
      label={t("shutdownRequirement.badge")}
      title={t("shutdownRequirement.help")}
      {...(className ? { className } : {})}
    />
  );
}
