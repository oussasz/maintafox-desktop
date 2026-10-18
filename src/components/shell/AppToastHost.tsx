import { X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { mfAlert } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useAppToastStore } from "@/store/app-toast-store";

export function AppToastHost() {
  const { t } = useTranslation("common");
  const items = useAppToastStore((s) => s.items);
  const dismiss = useAppToastStore((s) => s.dismiss);

  if (items.length === 0) {
    return null;
  }

  return (
    <div
      className="pointer-events-none fixed bottom-4 right-4 z-[200] flex max-w-sm flex-col gap-2"
      aria-live="polite"
    >
      {items.map((item) => (
        <div
          key={item.id}
          // A persistent warning is announced at once and stays until dismissed.
          role={item.persistent ? "alert" : undefined}
          className={cn(
            "pointer-events-auto shadow-panel",
            item.variant === "success"
              ? mfAlert.success
              : item.variant === "destructive"
                ? mfAlert.danger
                : mfAlert.info,
          )}
        >
          <div className="flex items-start gap-2">
            <p className="min-w-0 flex-1 font-medium text-text-primary">{item.title}</p>
            <button
              type="button"
              className="shrink-0 rounded p-0.5 text-text-muted hover:text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={t("toast.dismiss")}
              title={t("toast.dismiss")}
              onClick={() => dismiss(item.id)}
            >
              <X className="h-4 w-4" aria-hidden />
            </button>
          </div>
          {item.description ? (
            <p className="mt-1 text-xs text-text-secondary">{item.description}</p>
          ) : null}
        </div>
      ))}
    </div>
  );
}
