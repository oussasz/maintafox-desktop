/**
 * Lightweight anchored Popover (portaled overlay).
 * Avoids a new Radix dependency while matching the compact API used by Asset QR.
 */

import * as React from "react";
import { createPortal } from "react-dom";

import { cn } from "@/lib/utils";

type PopoverContextValue = {
  open: boolean;
  setOpen: (open: boolean) => void;
  triggerRef: React.MutableRefObject<HTMLElement | null>;
};

const PopoverContext = React.createContext<PopoverContextValue | null>(null);

function usePopoverContext(component: string): PopoverContextValue {
  const ctx = React.useContext(PopoverContext);
  if (!ctx) {
    throw new Error(`${component} must be used within <Popover>`);
  }
  return ctx;
}

type PopoverProps = {
  open?: boolean;
  defaultOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
  children: React.ReactNode;
};

function Popover({ open: openProp, defaultOpen = false, onOpenChange, children }: PopoverProps) {
  const [uncontrolledOpen, setUncontrolledOpen] = React.useState(defaultOpen);
  const isControlled = openProp !== undefined;
  const open = isControlled ? Boolean(openProp) : uncontrolledOpen;
  const triggerRef = React.useRef<HTMLElement | null>(null);

  const setOpen = React.useCallback(
    (next: boolean) => {
      if (!isControlled) setUncontrolledOpen(next);
      onOpenChange?.(next);
    },
    [isControlled, onOpenChange],
  );

  return (
    <PopoverContext.Provider value={{ open, setOpen, triggerRef }}>
      {children}
    </PopoverContext.Provider>
  );
}

type PopoverTriggerProps = React.ButtonHTMLAttributes<HTMLButtonElement> & {
  asChild?: boolean;
};

const PopoverTrigger = React.forwardRef<HTMLButtonElement, PopoverTriggerProps>(
  ({ asChild = false, onClick, children, ...props }, forwardedRef) => {
    const { open, setOpen, triggerRef } = usePopoverContext("PopoverTrigger");

    const assignRefs = (node: HTMLElement | null) => {
      triggerRef.current = node;
      if (typeof forwardedRef === "function") forwardedRef(node as HTMLButtonElement);
      else if (forwardedRef) {
        (forwardedRef as React.MutableRefObject<HTMLButtonElement | null>).current =
          node as HTMLButtonElement;
      }
    };

    const handleClick = (event: React.MouseEvent<HTMLButtonElement>) => {
      onClick?.(event);
      if (!event.defaultPrevented) setOpen(!open);
    };

    if (asChild && React.isValidElement(children)) {
      const child = children as React.ReactElement<{
        onClick?: (event: React.MouseEvent<HTMLElement>) => void;
      }>;
      return React.cloneElement(child, {
        ...props,
        ref: assignRefs,
        "aria-expanded": open,
        onClick: (event: React.MouseEvent<HTMLElement>) => {
          child.props.onClick?.(event);
          if (!event.defaultPrevented) setOpen(!open);
        },
      } as never);
    }

    return (
      <button
        type="button"
        {...props}
        ref={assignRefs as React.Ref<HTMLButtonElement>}
        aria-expanded={open}
        onClick={handleClick}
      >
        {children}
      </button>
    );
  },
);
PopoverTrigger.displayName = "PopoverTrigger";

type PopoverContentProps = React.HTMLAttributes<HTMLDivElement> & {
  align?: "start" | "center" | "end";
  side?: "top" | "bottom" | "left" | "right";
  sideOffset?: number;
};

const PopoverContent = React.forwardRef<HTMLDivElement, PopoverContentProps>(
  (
    {
      className,
      align = "center",
      side = "bottom",
      sideOffset = 8,
      style,
      children,
      ...props
    },
    forwardedRef,
  ) => {
    const { open, setOpen, triggerRef } = usePopoverContext("PopoverContent");
    const contentRef = React.useRef<HTMLDivElement | null>(null);
    const [coords, setCoords] = React.useState<{ top: number; left: number } | null>(null);

    const setRefs = React.useCallback(
      (node: HTMLDivElement | null) => {
        contentRef.current = node;
        if (typeof forwardedRef === "function") forwardedRef(node);
        else if (forwardedRef) forwardedRef.current = node;
      },
      [forwardedRef],
    );

    React.useLayoutEffect(() => {
      if (!open) {
        setCoords(null);
        return;
      }

      const update = () => {
        const trigger = triggerRef.current;
        const content = contentRef.current;
        if (!trigger || !content) return;

        const rect = trigger.getBoundingClientRect();
        const contentRect = content.getBoundingClientRect();
        let top = rect.bottom + sideOffset;
        let left = rect.left;

        if (side === "top") top = rect.top - contentRect.height - sideOffset;
        if (side === "left") {
          top = rect.top;
          left = rect.left - contentRect.width - sideOffset;
        }
        if (side === "right") {
          top = rect.top;
          left = rect.right + sideOffset;
        }

        if (side === "bottom" || side === "top") {
          if (align === "center") left = rect.left + rect.width / 2 - contentRect.width / 2;
          if (align === "end") left = rect.right - contentRect.width;
        }

        const maxLeft = window.innerWidth - contentRect.width - 8;
        const maxTop = window.innerHeight - contentRect.height - 8;
        left = Math.max(8, Math.min(left, maxLeft));
        top = Math.max(8, Math.min(top, maxTop));

        setCoords({ top, left });
      };

      update();
      const raf = window.requestAnimationFrame(update);
      window.addEventListener("resize", update);
      window.addEventListener("scroll", update, true);
      return () => {
        window.cancelAnimationFrame(raf);
        window.removeEventListener("resize", update);
        window.removeEventListener("scroll", update, true);
      };
    }, [align, open, side, sideOffset, triggerRef]);

    React.useEffect(() => {
      if (!open) return;

      const onKeyDown = (event: KeyboardEvent) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          setOpen(false);
        }
      };

      const onPointerDown = (event: MouseEvent) => {
        const target = event.target as Node;
        if (contentRef.current?.contains(target)) return;
        if (triggerRef.current?.contains(target)) return;
        setOpen(false);
      };

      document.addEventListener("keydown", onKeyDown);
      document.addEventListener("mousedown", onPointerDown);
      return () => {
        document.removeEventListener("keydown", onKeyDown);
        document.removeEventListener("mousedown", onPointerDown);
      };
    }, [open, setOpen, triggerRef]);

    if (!open || typeof document === "undefined") return null;

    return createPortal(
      <div
        {...props}
        ref={setRefs}
        role="dialog"
        data-side={side}
        className={cn(
          "z-50 w-auto rounded-md border border-surface-border bg-surface-1 p-4 text-text-primary shadow-md outline-none",
          className,
        )}
        style={{
          position: "fixed",
          top: coords?.top ?? -9999,
          left: coords?.left ?? -9999,
          visibility: coords ? "visible" : "hidden",
          ...style,
        }}
      >
        {children}
      </div>,
      document.body,
    );
  },
);
PopoverContent.displayName = "PopoverContent";

export { Popover, PopoverContent, PopoverTrigger };
