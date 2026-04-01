import "@testing-library/jest-dom";
import { vi } from "vitest";

// Mock the Tauri IPC runtime. Tests do not have access to the Tauri binary.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));
