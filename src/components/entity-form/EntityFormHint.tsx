import type { HTMLAttributes, ReactNode } from "react";

import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface EntityFormHintProps extends HTMLAttributes<HTMLDivElement> {
  children: ReactNode;
  variant?: "warning" | "info";
}

export function EntityFormHint({
  children,
  variant = "warning",
  className,
  ...props
}: EntityFormHintProps) {
  return (
    <div
      role="status"
      className={cn(variant === "info" ? mfEntityForm.hintInfo : mfEntityForm.hint, className)}
      {...props}
    >
      {children}
    </div>
  );
}
