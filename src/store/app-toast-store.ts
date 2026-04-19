import { create } from "zustand";

export interface AppToastItem {
  id: string;
  title: string;
  description?: string;
  variant?: "default" | "destructive" | "success";
}

interface AppToastState {
  items: AppToastItem[];
  push: (item: Omit<AppToastItem, "id">) => void;
}

export const useAppToastStore = create<AppToastState>((set) => ({
  items: [],
  push: (item): void => {
    const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    set((s) => ({ items: [...s.items, { ...item, id }] }));
    window.setTimeout(() => {
      set((s) => ({ items: s.items.filter((x) => x.id !== id) }));
    }, 6000);
  },
}));

export function pushAppToast(item: Omit<AppToastItem, "id">): void {
  useAppToastStore.getState().push(item);
}
