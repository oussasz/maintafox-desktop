import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { AuditChainCheck } from "../AuditChainCheck";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, o?: Record<string, unknown>) => (o ? `${key}:${JSON.stringify(o)}` : key),
    i18n: { language: "en" },
  }),
}));

const svc = vi.hoisted(() => ({ verifyAuditChain: vi.fn() }));
vi.mock("@/services/activity-service", () => svc);

describe("AuditChainCheck", () => {
  it("reports intact chains per device and the unchained count", async () => {
    svc.verifyAuditChain.mockResolvedValueOnce({
      checked_at: "2026-10-11T10:00:00Z",
      intact: true,
      unchained_events: 4,
      chains: [
        {
          origin: "r-1",
          device_code: "7KQ2",
          is_this_device: true,
          events: 3,
          last_seq: 3,
          kept_positions: 0,
          kept_before_stubs: 0,
          intact: true,
          first_problem: null,
        },
      ],
    });
    render(<AuditChainCheck />);
    fireEvent.click(screen.getByText("chain.verify"));
    const report = await screen.findByTestId("audit-chain-report");
    expect(report.textContent).toContain("chain.intact");
    expect(report.textContent).toContain('chain.thisDevice:{"code":"7KQ2"}');
    expect(report.textContent).toContain('chain.unchained:{"count":4}');
    expect(report.textContent).not.toContain("chain.kept");
  });

  it("counts the positions another device kept, linked or from before stubs", async () => {
    svc.verifyAuditChain.mockResolvedValueOnce({
      checked_at: "2026-10-11T10:00:00Z",
      intact: true,
      unchained_events: 0,
      chains: [
        {
          origin: "r-3",
          device_code: "CD34",
          is_this_device: false,
          events: 6,
          last_seq: 10,
          kept_positions: 3,
          kept_before_stubs: 1,
          intact: true,
          first_problem: null,
        },
      ],
    });
    render(<AuditChainCheck />);
    fireEvent.click(screen.getByText("chain.verify"));
    const report = await screen.findByTestId("audit-chain-report");
    expect(report.textContent).toContain("chain.intact");
    expect(report.textContent).toContain('chain.kept:{"count":3}');
    expect(report.textContent).toContain('chain.keptBeforeStubs:{"count":1}');
  });

  it("names a broken position kept on its device without an event number", async () => {
    svc.verifyAuditChain.mockResolvedValueOnce({
      checked_at: "2026-10-11T10:00:00Z",
      intact: false,
      unchained_events: 0,
      chains: [
        {
          origin: "r-4",
          device_code: "EF56",
          is_this_device: false,
          events: 2,
          last_seq: 3,
          kept_positions: 0,
          kept_before_stubs: 0,
          intact: false,
          first_problem: { chain_seq: 2, audit_event_id: null, kind: "prev_mismatch" },
        },
      ],
    });
    render(<AuditChainCheck />);
    fireEvent.click(screen.getByText("chain.verify"));
    const report = await screen.findByTestId("audit-chain-report");
    expect(report.textContent).toContain('chain.problemKept.prev_mismatch:{"seq":2}');
  });

  it("names the first problem of a broken chain", async () => {
    svc.verifyAuditChain.mockResolvedValueOnce({
      checked_at: "2026-10-11T10:00:00Z",
      intact: false,
      unchained_events: 0,
      chains: [
        {
          origin: "r-2",
          device_code: "AB12",
          is_this_device: false,
          events: 5,
          last_seq: 5,
          kept_positions: 0,
          kept_before_stubs: 0,
          intact: false,
          first_problem: { chain_seq: 2, audit_event_id: 41, kind: "hash_mismatch" },
        },
      ],
    });
    render(<AuditChainCheck />);
    fireEvent.click(screen.getByText("chain.verify"));
    const report = await screen.findByTestId("audit-chain-report");
    expect(report.textContent).toContain("chain.broken");
    expect(report.textContent).toContain('chain.problem.hash_mismatch:{"seq":2,"id":41}');
  });

  it("shows verification errors", async () => {
    svc.verifyAuditChain.mockRejectedValueOnce(new Error("permission denied"));
    render(<AuditChainCheck />);
    fireEvent.click(screen.getByText("chain.verify"));
    expect((await screen.findByRole("alert")).textContent).toContain("permission denied");
  });
});
