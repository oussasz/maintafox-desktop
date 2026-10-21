/**
 * App-wide messages: timed ones go by themselves; a persistent warning stays, announced as an
 * alert, until the user dismisses it.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AppToastHost } from "@/components/shell/AppToastHost";
import { pushAppToast, useAppToastStore } from "@/store/app-toast-store";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

beforeEach(() => {
  vi.useFakeTimers();
  useAppToastStore.setState({ items: [] });
});
afterEach(() => vi.useRealTimers());

describe("AppToastHost", () => {
  it("keeps a persistent warning until it is dismissed", () => {
    render(<AppToastHost />);
    act(() => {
      pushAppToast({ title: "Saved", variant: "success" });
      pushAppToast({ title: "Rules not applied", variant: "destructive", persistent: true });
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Rules not applied");

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.queryByText("Saved")).toBeNull();
    expect(screen.getByText("Rules not applied")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "toast.dismiss" }));
    expect(screen.queryByText("Rules not applied")).toBeNull();
  });
});
