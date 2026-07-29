import type { HTMLAttributes, ReactNode } from "react";

import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface EntityFormSectionProps extends Omit<HTMLAttributes<HTMLElement>, "title"> {
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}

export function EntityFormSection({
  title,
  description,
  children,
  className,
  ...props
}: EntityFormSectionProps) {
  return (
    <section className={cn(mfEntityForm.section, className)} {...props}>
      <div className="space-y-1">
        <h3 className={mfEntityForm.sectionTitle}>{title}</h3>
        {description != null && (
          <p className={mfEntityForm.sectionDescription}>{description}</p>
        )}
      </div>
      <div className="space-y-3">{children}</div>
    </section>
  );
}
