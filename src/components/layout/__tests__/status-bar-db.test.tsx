/**
 * The status bar shows the database state the backend's probe reported: OK with its latency,
 * a visible degraded state with the reason, and "unknown" (never green) when the probe cannot be
 * asked.
 */
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { StatusBar } from "@/components/layout/StatusBar";

const i18nMock = vi.hoisted(() => ({
  t: (key: string, opts?: Record<string, unknown>) =>
    opts && "ms" in opts ? `${key}:${String(opts["ms"])}` : key,
  i18n: { language: "en" },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => i18nMock }));

const app = vi.hoisted(() => ({ healthCheck: vi.fn() }));
vi.mock("@/services/app.service", () => app);

const BASE = { version: "0.4.0", locale: "fr" };

describe("status bar database indicator", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows OK with the probe's latency", async () => {
    app.healthCheck.mockResolvedValue({
      ...BASE,
      status: "ok",
      db_connected: true,
      db_latency_ms: 12,
    });
    render(<StatusBar />);
    const db = await screen.findByText("status.dbOkLatency:12");
    expect(db.closest("[data-testid=db-health]")).toHaveAttribute("data-state", "ok");
  });

  it("makes a degraded database visible with its reason", async () => {
    app.healthCheck.mockResolvedValue({
      ...BASE,
      status: "degraded",
      db_connected: false,
      db_error_code: "DB_TIMEOUT",
    });
    render(<StatusBar />);
    const db = await screen.findByText("status.dbTimeout");
    expect(db.closest("[data-testid=db-health]")).toHaveAttribute("data-state", "degraded");
  });

  it("says unknown, never OK, when the probe cannot be asked", async () => {
    app.healthCheck.mockRejectedValue(new Error("bridge down"));
    render(<StatusBar />);
    expect(await screen.findByText("status.dbUnknown")).toBeInTheDocument();
    expect(screen.queryByText(/status\.dbOk/)).not.toBeInTheDocument();
  });
});
