import type { HTMLAttributes, ReactNode } from "react";

import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface EntityFormFieldGroupProps extends Omit<HTMLAttributes<HTMLDivElement>, "title"> {
  title?: ReactNode;
  children: ReactNode;
}

/** Visually tight group for cascading / related fields (e.g. Class → Family → Subfamily). */
export function EntityFormFieldGroup({
  title,
  children,
  className,
  ...props
}: EntityFormFieldGroupProps) {
  return (
    <div className={cn(mfEntityForm.fieldGroup, className)} {...props}>
      {title != null && <div className={mfEntityForm.fieldGroupTitle}>{title}</div>}
      <div className="space-y-3">{children}</div>
    </div>
  );
}
