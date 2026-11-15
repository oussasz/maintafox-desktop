import { create } from "zustand";

export interface AppToastItem {
  id: string;
  title: string;
  description?: string;
  variant?: "default" | "destructive" | "success";
  /** Stays until the user dismisses it (a warning that must not be missed). */
  persistent?: boolean;
  /** A follow-up the message offers (e.g. "Open" the record it just created). */
  action?: { label: string; onClick: () => void };
}

interface AppToastState {
  items: AppToastItem[];
  push: (item: Omit<AppToastItem, "id">) => void;
  dismiss: (id: string) => void;
  /** Keeps a toast on screen while the pointer is on it (it is being read). */
  hold: (id: string) => void;
  /** Restarts the countdown of a held toast. */
  release: (id: string) => void;
  /** Removes the toasts that say exactly this (the failure is now shown on the screen). */
  dismissMatching: (title: string, description?: string | null) => void;
}

const timers = new Map<string, number>();

/** A confirmation stays 8 s, a refusal or failure 12 s (read, understood and acted on). */
function lifetimeMs(variant: AppToastItem["variant"]): number {
  return variant === "destructive" ? 12000 : 8000;
}

function schedule(id: string, ms: number, remove: (id: string) => void): void {
  const previous = timers.get(id);
  if (previous !== undefined) window.clearTimeout(previous);
  timers.set(
    id,
    window.setTimeout(() => {
      timers.delete(id);
      remove(id);
    }, ms),
  );
}

export const useAppToastStore = create<AppToastState>((set, get) => ({
  items: [],
  push: (item): void => {
    // The same message shown twice (one refusal reported by two callers) is shown once.
    const shown = useAppToastStore
      .getState()
      .items.some(
        (x) =>
          x.title === item.title &&
          x.description === item.description &&
          x.variant === item.variant,
      );
    if (shown) return;
    const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    set((s) => ({ items: [...s.items, { ...item, id }] }));
    if (item.persistent) return;
    schedule(id, lifetimeMs(item.variant), get().dismiss);
  },
  dismiss: (id): void => {
    const timer = timers.get(id);
    if (timer !== undefined) window.clearTimeout(timer);
    timers.delete(id);
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
  },
  hold: (id): void => {
    const timer = timers.get(id);
    if (timer !== undefined) window.clearTimeout(timer);
    timers.delete(id);
  },
  release: (id): void => {
    const item = get().items.find((x) => x.id === id);
    if (!item || item.persistent) return;
    schedule(id, 4000, get().dismiss);
  },
  dismissMatching: (title, description): void => {
    set((s) => {
      const items = s.items.filter(
        (x) => !(x.title === title && (x.description ?? null) === (description ?? null)),
      );
      return items.length === s.items.length ? s : { items };
    });
  },
}));

export function pushAppToast(item: Omit<AppToastItem, "id">): void {
  useAppToastStore.getState().push(item);
}
