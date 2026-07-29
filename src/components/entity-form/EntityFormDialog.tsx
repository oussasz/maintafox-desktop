import * as React from "react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { mfEntityForm } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export type EntityFormDialogSize = "form" | "wide";

export interface EntityFormDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: React.ReactNode;
  description?: React.ReactNode;
  children: React.ReactNode;
  footer: React.ReactNode;
  size?: EntityFormDialogSize;
  /** Form element id when footer submit is outside the form. */
  formId?: string;
  className?: string;
  contentClassName?: string;
}

/**
 * Official Create/Edit dialog shell: header, scrollable body, sticky footer.
 * @see docs/UX_ENTITY_FORM_DIALOG_PATTERN.md
 */
export function EntityFormDialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  size = "form",
  className,
  contentClassName,
}: EntityFormDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className={cn(
          size === "wide" ? mfEntityForm.dialogWide : mfEntityForm.dialog,
          contentClassName,
        )}
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader className={cn(mfEntityForm.header, className)}>
          <DialogTitle className={mfEntityForm.title}>{title}</DialogTitle>
          {description != null && description !== false && (
            <DialogDescription className={mfEntityForm.description}>
              {description}
            </DialogDescription>
          )}
        </DialogHeader>

        <div className={mfEntityForm.body}>{children}</div>

        <div className={mfEntityForm.footer}>{footer}</div>
      </DialogContent>
    </Dialog>
  );
}
