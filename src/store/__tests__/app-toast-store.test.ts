import { afterEach, describe, expect, it, vi } from "vitest";

import { pushAppToast, useAppToastStore } from "@/store/app-toast-store";

describe("app toast store", () => {
  afterEach(() => {
    vi.useRealTimers();
    useAppToastStore.setState({ items: [] });
  });

  it("shows the same message once when two callers report one refusal", () => {
    pushAppToast({
      title: "Permission refusée",
      description: "Droit requis",
      variant: "destructive",
    });
    pushAppToast({
      title: "Permission refusée",
      description: "Droit requis",
      variant: "destructive",
    });
    expect(useAppToastStore.getState().items).toHaveLength(1);
  });

  it("keeps different messages apart", () => {
    pushAppToast({ title: "Enregistré", variant: "success" });
    pushAppToast({ title: "Permission refusée", variant: "destructive" });
    expect(useAppToastStore.getState().items).toHaveLength(2);
  });

  it("keeps a failure on screen longer than a confirmation", () => {
    vi.useFakeTimers();
    pushAppToast({ title: "Enregistré", variant: "success" });
    pushAppToast({ title: "Échec", variant: "destructive" });
    vi.advanceTimersByTime(9000);
    expect(useAppToastStore.getState().items.map((i) => i.title)).toEqual(["Échec"]);
    vi.advanceTimersByTime(4000);
    expect(useAppToastStore.getState().items).toHaveLength(0);
  });

  it("keeps a toast while it is held and lets it go a few seconds after release", () => {
    vi.useFakeTimers();
    pushAppToast({ title: "OT-0042 créé", variant: "success" });
    const id = useAppToastStore.getState().items[0]?.id ?? "";
    useAppToastStore.getState().hold(id);
    vi.advanceTimersByTime(30000);
    expect(useAppToastStore.getState().items).toHaveLength(1);
    useAppToastStore.getState().release(id);
    vi.advanceTimersByTime(4100);
    expect(useAppToastStore.getState().items).toHaveLength(0);
  });
});
