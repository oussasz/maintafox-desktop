import { render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useCompactAuthWindow } from "@/hooks/use-compact-auth-window";
import { mockInvoke } from "@/test/mocks/tauri";

function Screen() {
  useCompactAuthWindow();
  return null;
}

const modes = () =>
  mockInvoke.mock.calls.filter(([cmd]) => cmd === "set_window_mode").map(([, args]) => args);

beforeEach(() => {
  vi.useFakeTimers();
  mockInvoke.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useCompactAuthWindow", () => {
  it("asks for the compact window while a pre-login screen is shown, then the main window", async () => {
    const first = render(<Screen />);
    await vi.advanceTimersByTimeAsync(0);
    expect(modes()).toEqual([{ mode: "compact" }]);
    first.unmount();
    await vi.advanceTimersByTimeAsync(400);
    expect(modes()).toEqual([{ mode: "compact" }, { mode: "main" }]);
  });

  it("does not resize back and forth when one pre-login screen replaces another", async () => {
    const a = render(<Screen />);
    await vi.advanceTimersByTimeAsync(0);
    mockInvoke.mockClear();
    a.unmount();
    const b = render(<Screen />);
    await vi.advanceTimersByTimeAsync(400);
    expect(modes()).toEqual([]);
    b.unmount();
    await vi.advanceTimersByTimeAsync(400);
    expect(modes()).toEqual([{ mode: "main" }]);
  });
});
