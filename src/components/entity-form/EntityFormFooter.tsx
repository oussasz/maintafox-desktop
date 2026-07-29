import type { HTMLAttributes, ReactNode } from "react";

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
    </div>
  );
}
