import { mfAlert } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useAppToastStore } from "@/store/app-toast-store";

export function AppToastHost() {
  const items = useAppToastStore((s) => s.items);

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
          className={cn(
            "pointer-events-auto shadow-panel",
            item.variant === "success"
              ? mfAlert.success
              : item.variant === "destructive"
                ? mfAlert.danger
                : mfAlert.info,
          )}
        >
          <p className="font-medium text-text-primary">{item.title}</p>
          {item.description ? (
            <p className="mt-1 text-xs text-text-secondary">{item.description}</p>
          ) : null}
        </div>
      ))}
    </div>
  );
}
