import "@testing-library/jest-dom";
import { cleanup } from "@testing-library/react";
import { vi, afterEach } from "vitest";

// Mock the Tauri IPC runtime. Tests do not have access to the Tauri binary.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
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

afterEach(() => {
  cleanup();
  // Reset localStorage to prevent Zustand persist cross-contamination
  localStorage.clear();
});
