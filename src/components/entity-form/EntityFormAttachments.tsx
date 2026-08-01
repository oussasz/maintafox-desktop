import { FileText, Paperclip, Trash2, Upload } from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export interface EntityFormAttachmentItem {
  id: string;
  name: string;
  meta?: string | null;
}

export interface EntityFormAttachmentsProps {
  items: EntityFormAttachmentItem[];
  onAdd?: () => void;
  onRemove?: (id: string) => void;
  disabled?: boolean;
  emptyLabel?: ReactNode;
  addLabel?: ReactNode;
  removeLabel?: ReactNode;
  /** When true, hide add until the entity exists (create deferred pattern). */
  deferred?: boolean;
  deferredLabel?: ReactNode;
  className?: string;
}

/**
 * Generic attachments list for entity forms (PDF / manuals / etc.).
 * Props-driven — no entity-specific APIs.
 */
export function EntityFormAttachments({
  items,
  onAdd,
  onRemove,
  disabled = false,
  emptyLabel = "No attachments",
  addLabel = "Add file",
  removeLabel = "Remove",
  deferred = false,
  deferredLabel = "Attachments will be available after the record is created.",
  className,
}: EntityFormAttachmentsProps) {
  if (deferred) {
    return (
      <div
        className={cn(
          "rounded-md border border-dashed border-surface-border bg-surface-2/30 px-3 py-4 text-sm text-text-muted",
          className,
        )}
      >
        <div className="flex items-start gap-2">
          <Paperclip className="mt-0.5 h-4 w-4 shrink-0" />
          <p>{deferredLabel}</p>
        </div>
      </div>
    );
  }

  return (
    <div className={cn("space-y-3", className)}>
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-text-muted">{items.length === 0 ? emptyLabel : null}</span>
        {onAdd && (
          <Button type="button" variant="outline" size="sm" disabled={disabled} onClick={onAdd}>
            <Upload className="mr-1.5 h-3.5 w-3.5" />
            {addLabel}
          </Button>
        )}
      </div>

      {items.length > 0 && (
        <ul className="divide-y divide-surface-border rounded-md border border-surface-border">
          {items.map((item) => (
            <li
              key={item.id}
              className="flex items-center gap-2 px-3 py-2 text-sm text-text-primary"
            >
              <FileText className="h-4 w-4 shrink-0 text-text-muted" />
              <div className="min-w-0 flex-1">
                <p className="truncate font-medium">{item.name}</p>
                {item.meta ? <p className="truncate text-xs text-text-muted">{item.meta}</p> : null}
              </div>
              {onRemove && (
                <Button
                  type="button"
                  size="icon"
                  variant="ghost"
                  className="h-8 w-8 shrink-0"
                  disabled={disabled}
                  title={String(removeLabel)}
                  onClick={() => onRemove(item.id)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  <span className="sr-only">{removeLabel}</span>
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
