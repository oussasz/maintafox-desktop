import { create } from "zustand";

export interface AppToastItem {
  id: string;
  title: string;
  description?: string;
  variant?: "default" | "destructive" | "success";
  /** Stays until the user dismisses it (a warning that must not be missed). */
  persistent?: boolean;
}

interface AppToastState {
  items: AppToastItem[];
  push: (item: Omit<AppToastItem, "id">) => void;
  dismiss: (id: string) => void;
}

export const useAppToastStore = create<AppToastState>((set) => ({
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
    // A refusal or a failure is read, understood and acted on: it stays longer than a confirmation.
    window.setTimeout(
      () => {
        set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
      },
      item.variant === "destructive" ? 12000 : 6000,
    );
  },
  dismiss: (id): void => {
    set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
  },
}));

export function pushAppToast(item: Omit<AppToastItem, "id">): void {
  useAppToastStore.getState().push(item);
}
