/**
 * The worker card says why sync failed in translated words (AU-44): the raw transport text
 * ("HTTP 503: …", the network library's) stays in the technical details.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "message" in opts ? `${key}::${String(opts["message"])}` : key,
    i18n: { language: "fr", changeLanguage: vi.fn() },
  }),
}));

vi.mock("@/hooks/use-formatters", () => ({
  useFormatters: () => ({
    formatDateTime: (v: string) => v,
    formatNumber: (n: number) => String(n),
  }),
}));

const worker = {
  state: "retrying",
  blocker: null,
  paused_until: null,
  last_success_at: null,
  last_attempt_at: null,
  next_attempt_at: null,
  pending_outbox_count: 1,
  dead_letter_outbox_count: 0,
  unresolved_conflict_count: 0,
  media_pending_count: 0,
  media_failed_count: 0,
  media_download_pending_count: 0,
  media_download_failed_count: 0,
  consecutive_failures: 2,
  last_error: {
    code: "http_error",
    kind: "server_unavailable",
    message: "HTTP 503: upstream connect error",
    http_status: 503,
    at: "2026-10-26T08:00:00Z",
  },
};

vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (sel: (s: unknown) => unknown) =>
    sel({
      worker,
      actionError: null,
      runSyncNow: vi.fn(),
      pauseForMinutes: vi.fn(),
      resumeNow: vi.fn(),
    }),
  syncBlockerText: () => null,
  syncWorkerErrorText: (e: { kind: string }) => `errorKind.${e.kind}`,
}));

import { SyncWorkerStatusCard } from "../SyncWorkerStatusCard";

describe("SyncWorkerStatusCard", () => {
  it("shows the translated reason and keeps the raw HTTP text in the technical details", () => {
    render(<SyncWorkerStatusCard />);
    const line = screen.getByText("sync.worker.lastError::errorKind.server_unavailable");
    expect(line.textContent).not.toContain("HTTP 503");
    const raw = screen.getByText(/HTTP 503: upstream connect error/);
    expect(raw.closest("details")).not.toBeNull();
  });
});
