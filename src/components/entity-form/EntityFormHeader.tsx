import type { ReactNode } from "react";

import { DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface EntityFormHeaderProps {
  title: ReactNode;
  description?: ReactNode;
  className?: string;
}

/** Standalone header when composing outside `EntityFormDialog`. */
export function EntityFormHeader({ title, description, className }: EntityFormHeaderProps) {
  return (
    <DialogHeader className={cn(mfEntityForm.header, className)}>
      <DialogTitle className={mfEntityForm.title}>{title}</DialogTitle>
      {description != null && (
        <DialogDescription className={mfEntityForm.description}>{description}</DialogDescription>
      )}
    </DialogHeader>
  );
}
