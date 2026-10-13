import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    coverage: {
      provider: "v8",
      include: ["src/services/**", "src/hooks/**", "src/store/**", "src/stores/**"],
      exclude: ["src/test/**", "**/*.d.ts"],
      // Ratchet, not target: floors at measured coverage so it cannot regress. 2026-10-12, with
      // src/stores (the zustand stores) now included and tested: lines/statements 62.49%,
      // functions 55.24%, branches 93.27%. Raise these as tests are added; never lower them.
      thresholds: {
        lines: 62,
        functions: 55,
        branches: 93,
        statements: 62,
      },
      reporter: ["text", "lcov"],
    },
  },
  resolve: {
    alias: {
      "@": resolve(__dirname, "./src"),
      "@shared": resolve(__dirname, "./shared"),
    },
  },
});
