import { Paperclip } from "lucide-react";
import * as React from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";

export type FileInputProps = Omit<React.InputHTMLAttributes<HTMLInputElement>, "type">;

/**
 * File picker in the interface language. The native `<input type="file">` shows the WebView's own
 * "Choose File / No file chosen" (English on most PCs, L6-12): the input stays (keyboard, label,
 * `accept`, `multiple`, form reset by `key`) but is hidden behind a translated button and the name
 * of the chosen file.
 */
export const FileInput = React.forwardRef<HTMLInputElement, FileInputProps>(
  ({ className, onChange, disabled, multiple, ...props }, ref) => {
    const { t } = useTranslation("common");
    const [names, setNames] = React.useState<string[]>([]);
    return (
      <label
        className={cn(
          "inline-flex min-h-10 max-w-full cursor-pointer items-center gap-3 text-sm",
          disabled && "cursor-not-allowed opacity-60",
          className,
        )}
      >
        <input
          ref={ref}
          type="file"
          className="peer sr-only"
          disabled={disabled}
          multiple={multiple}
          onChange={(e) => {
            setNames(Array.from(e.target.files ?? []).map((f) => f.name));
            onChange?.(e);
          }}
          {...props}
        />
        <span
          aria-hidden
          className="inline-flex h-10 shrink-0 items-center gap-1.5 rounded-md border border-surface-border bg-surface-1 px-3 font-medium text-text-primary hover:bg-surface-2 peer-focus-visible:ring-2 peer-focus-visible:ring-ring"
        >
          <Paperclip className="h-4 w-4" />
          {multiple ? t("fileInput.chooseMany") : t("fileInput.choose")}
        </span>
        <span className="min-w-0 truncate text-text-secondary">
          {names.length === 0
            ? t("fileInput.none")
            : names.length === 1
              ? names[0]
              : t("fileInput.many", { count: names.length })}
        </span>
      </label>
    );
  },
);
FileInput.displayName = "FileInput";
