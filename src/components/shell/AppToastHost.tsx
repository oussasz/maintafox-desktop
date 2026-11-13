import { CheckCircle2, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { mfAlert } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useAppToastStore } from "@/store/app-toast-store";

export function AppToastHost() {
  const { t } = useTranslation("common");
  const items = useAppToastStore((s) => s.items);
  const dismiss = useAppToastStore((s) => s.dismiss);
  const hold = useAppToastStore((s) => s.hold);
  const release = useAppToastStore((s) => s.release);

  if (items.length === 0) {
    return null;
  }

  return (
    <div
      // Top right, under the header: where the eye is after clicking "Enregistrer" / "Créer".
      className="pointer-events-none fixed right-4 top-16 z-[200] flex w-[min(28rem,calc(100vw-2rem))] flex-col gap-2"
      aria-live="polite"
    >
      {items.map((item) => (
        <div
          key={item.id}
          // A persistent warning is announced at once and stays until dismissed.
          role={item.persistent ? "alert" : undefined}
          onMouseEnter={() => hold(item.id)}
          onMouseLeave={() => release(item.id)}
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
            {item.variant === "success" ? (
              <CheckCircle2 className="mt-0.5 h-5 w-5 shrink-0 text-status-success" aria-hidden />
            ) : null}
            <p className="min-w-0 flex-1 text-[15px] font-semibold text-text-primary">
              {item.title}
            </p>
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
            <p className="mt-1 text-sm text-text-secondary">{item.description}</p>
          ) : null}
          {item.action ? (
            <button
              type="button"
              className="mt-1 text-sm font-medium text-primary underline underline-offset-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() => {
                item.action?.onClick();
                dismiss(item.id);
              }}
            >
              {item.action.label}
            </button>
          ) : null}
        </div>
      ))}
    </div>
  );
}
