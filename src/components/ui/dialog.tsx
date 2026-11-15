import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import * as React from "react";
import { useTranslation } from "react-i18next";

import { mfModal } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

const Dialog = DialogPrimitive.Root;

const DialogTrigger = DialogPrimitive.Trigger;

const DialogPortal = DialogPrimitive.Portal;

const DialogClose = DialogPrimitive.Close;

const DialogOverlay = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Overlay>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Overlay>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Overlay
    ref={ref}
    className={cn(
      mfModal.overlay,
      "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0",
      className,
    )}
    {...props}
  />
));
DialogOverlay.displayName = DialogPrimitive.Overlay.displayName;

/** Typed text in a field the user would lose on closing (search boxes and read-only fields excluded). */
function isTypedField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.closest("[data-dirty-ignore]")) return false;
  if (target instanceof HTMLTextAreaElement) return !target.readOnly;
  if (target instanceof HTMLInputElement) {
    return !target.readOnly && !["search", "checkbox", "radio", "range"].includes(target.type);
  }
  return target.isContentEditable;
}

/** True while a typed field inside this dialog (not in a popup it opened) holds text. */
function hasTypedText(root: HTMLElement): boolean {
  return Array.from(root.querySelectorAll<HTMLElement>("input, textarea, [contenteditable]")).some(
    (el) =>
      isTypedField(el) &&
      ((el as HTMLInputElement).value ?? el.textContent ?? "").trim().length > 0,
  );
}

type DialogContentProps = React.ComponentPropsWithoutRef<typeof DialogPrimitive.Content> & {
  /**
   * Ask before Escape, a click outside or the cross close the dialog while the user has typed
   * something ("Quitter sans enregistrer ?"). On by default; pass `false` for dialogs that only
   * hold a search or a confirmation.
   */
  confirmDiscard?: boolean;
};

const DialogContent = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Content>,
  DialogContentProps
>(
  (
    {
      className,
      children,
      confirmDiscard = true,
      onEscapeKeyDown,
      onInteractOutside,
      onInputCapture,
      ...props
    },
    ref,
  ) => {
    const { t } = useTranslation("common");
    const closeLabel = t("action.close");
    const [dirty, setDirty] = React.useState(false);
    const [asking, setAsking] = React.useState(false);
    const guarded = confirmDiscard && dirty;
    const contentEl = React.useRef<HTMLDivElement | null>(null);
    const descriptionId = React.useId();
    const setRefs = React.useCallback(
      (node: HTMLDivElement | null) => {
        contentEl.current = node;
        if (typeof ref === "function") ref(node);
        else if (ref) ref.current = node;
      },
      [ref],
    );

    // While the question shows, the fields behind it cannot be reached by keyboard or reader.
    React.useEffect(() => {
      const root = contentEl.current;
      if (!asking || !root) return;
      const behind = Array.from(root.children).filter((el) => !el.hasAttribute("data-discard-ask"));
      for (const el of behind) el.setAttribute("inert", "");
      return () => {
        for (const el of behind) el.removeAttribute("inert");
      };
    }, [asking]);

    const keepEditing = () => {
      setAsking(false);
      requestAnimationFrame(() =>
        contentEl.current
          ?.querySelector<HTMLElement>("input:not([type=search]), textarea")
          ?.focus(),
      );
    };

    return (
      <DialogPortal>
        <DialogOverlay />
        <DialogPrimitive.Content
          ref={setRefs}
          className={cn(
            "fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]",
            mfModal.content,
            className,
          )}
          {...props}
          onInputCapture={(e) => {
            onInputCapture?.(e);
            const root = e.currentTarget;
            // Typing in a popup this dialog opened (a list filter, a second dialog) bubbles here
            // through the React tree but is not in this dialog.
            if (!confirmDiscard || !(e.target instanceof Node) || !root.contains(e.target)) return;
            if (isTypedField(e.target)) setDirty(hasTypedText(root));
          }}
          onEscapeKeyDown={(e) => {
            onEscapeKeyDown?.(e);
            if (asking) {
              e.preventDefault();
              keepEditing();
              return;
            }
            if (e.defaultPrevented || !guarded) return;
            e.preventDefault();
            setAsking(true);
          }}
          onInteractOutside={(e) => {
            onInteractOutside?.(e);
            if (e.defaultPrevented || !guarded) return;
            e.preventDefault();
            setAsking(true);
          }}
        >
          {children}
          {guarded ? (
            <button
              type="button"
              className="absolute right-3 top-3 inline-flex h-9 w-9 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-surface-2 hover:text-text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={closeLabel}
              title={closeLabel}
              onClick={() => setAsking(true)}
            >
              <X className="h-4 w-4" aria-hidden />
            </button>
          ) : (
            <DialogPrimitive.Close
              className="absolute right-3 top-3 inline-flex h-9 w-9 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-surface-2 hover:text-text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none"
              aria-label={closeLabel}
              title={closeLabel}
            >
              <X className="h-4 w-4" aria-hidden />
            </DialogPrimitive.Close>
          )}
          {asking ? (
            <div
              role="alertdialog"
              aria-modal="true"
              aria-label={t("discardGuard.title")}
              aria-describedby={descriptionId}
              data-discard-ask=""
              className="absolute inset-0 z-10 flex items-center justify-center rounded-[inherit] bg-background/90 p-6"
            >
              <div className="max-w-sm space-y-4 text-center">
                <p className="text-base font-semibold">{t("discardGuard.title")}</p>
                <p id={descriptionId} className="text-sm text-text-muted">
                  {t("discardGuard.description")}
                </p>
                <div className="flex justify-center gap-2">
                  <button
                    type="button"
                    autoFocus
                    className="inline-flex h-9 items-center rounded-md border border-surface-border px-4 text-sm hover:bg-surface-2"
                    onClick={keepEditing}
                  >
                    {t("discardGuard.keep")}
                  </button>
                  <DialogPrimitive.Close className="inline-flex h-9 items-center rounded-md bg-destructive px-4 text-sm text-white hover:opacity-90">
                    {t("discardGuard.leave")}
                  </DialogPrimitive.Close>
                </div>
              </div>
            </div>
          ) : null}
        </DialogPrimitive.Content>
      </DialogPortal>
    );
  },
);
DialogContent.displayName = DialogPrimitive.Content.displayName;

const DialogHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("flex flex-col space-y-1.5 text-center sm:text-left", className)} {...props} />
);
DialogHeader.displayName = "DialogHeader";

const DialogFooter = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div
    className={cn("flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-2", className)}
    {...props}
  />
);
DialogFooter.displayName = "DialogFooter";

const DialogTitle = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Title>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Title>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Title
    ref={ref}
    className={cn("text-lg font-semibold leading-none tracking-tight", className)}
    {...props}
  />
));
DialogTitle.displayName = DialogPrimitive.Title.displayName;

const DialogDescription = React.forwardRef<
  React.ElementRef<typeof DialogPrimitive.Description>,
  React.ComponentPropsWithoutRef<typeof DialogPrimitive.Description>
>(({ className, ...props }, ref) => (
  <DialogPrimitive.Description
    ref={ref}
    className={cn("text-sm text-muted-foreground", className)}
    {...props}
  />
));
DialogDescription.displayName = DialogPrimitive.Description.displayName;

export {
  Dialog,
  DialogPortal,
  DialogOverlay,
  DialogClose,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
};
