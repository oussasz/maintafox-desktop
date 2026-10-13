import "@testing-library/jest-dom";
import { cleanup } from "@testing-library/react";
import { vi, afterEach } from "vitest";

// Mock the Tauri IPC runtime. Tests do not have access to the Tauri binary.
// `Channel` keeps the handler it is given so tests can push messages through `onmessage`
// (the backend side of a channel), as the real Tauri Channel does.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
  Channel: class Channel<T = unknown> {
    onmessage: (response: T) => void;
    constructor(onmessage?: (response: T) => void) {
      this.onmessage = onmessage ?? (() => undefined);
    }
  },
}));

// Stub ResizeObserver which is not available in jsdom.
// Chart components and responsive containers depend on it.
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof globalThis.ResizeObserver;
}

// jsdom has no layout: Radix Select / menus call scrollIntoView on the highlighted item.
if (typeof Element.prototype.scrollIntoView !== "function") {
  Element.prototype.scrollIntoView = function scrollIntoView() {};
}

afterEach(() => {
  cleanup();
  // Reset localStorage to prevent Zustand persist cross-contamination
  localStorage.clear();
});
