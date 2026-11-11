import type { HTMLAttributes, ReactNode } from "react";

import { ActionReason } from "@/components/ui/action-reason";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface EntityFormFooterProps extends HTMLAttributes<HTMLDivElement> {
  cancelLabel: ReactNode;
  primaryLabel: ReactNode;
  onCancel: () => void;
  /** When set, primary is a submit button for this form id. */
  formId?: string;
  onPrimary?: () => void;
  primaryDisabled?: boolean;
  cancelDisabled?: boolean;
  primaryLoading?: boolean;
  primaryType?: "button" | "submit";
  leading?: ReactNode;
  /** Why the primary button is disabled, in plain words; shown beside the buttons. */
  primaryDisabledReason?: ReactNode;
}

export function EntityFormFooter({
  cancelLabel,
  primaryLabel,
  onCancel,
  formId,
  onPrimary,
  primaryDisabled,
  cancelDisabled,
  primaryLoading,
  primaryType = "submit",
  leading,
  primaryDisabledReason,
  className,
  ...props
}: EntityFormFooterProps) {
  return (
    <div className={cn("contents", className)} {...props}>
      {leading}
      <Button type="button" variant="outline" onClick={onCancel} disabled={cancelDisabled}>
        {cancelLabel}
      </Button>
      <Button
        type={primaryType}
        form={formId}
        onClick={onPrimary}
        disabled={primaryDisabled || primaryLoading}
      >
        {primaryLabel}
      </Button>
      <ActionReason className="sm:order-first sm:mr-auto sm:self-center">
        {primaryDisabledReason}
      </ActionReason>
    </div>
  );
}
