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
    vi.advanceTimersByTime(7000);
    expect(useAppToastStore.getState().items.map((i) => i.title)).toEqual(["Échec"]);
    vi.advanceTimersByTime(6000);
    expect(useAppToastStore.getState().items).toHaveLength(0);
  });
});
