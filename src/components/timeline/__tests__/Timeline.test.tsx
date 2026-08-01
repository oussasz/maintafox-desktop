import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Timeline } from "@/components/timeline/Timeline";
import type { TimelineEntry } from "@/components/timeline/types";

describe("Timeline", () => {
  it("renders empty state", () => {
    render(<Timeline items={[]} empty="Nothing here" />);
    expect(screen.getByText("Nothing here")).toBeInTheDocument();
  });

  it("renders loading skeleton", () => {
    const { container } = render(<Timeline items={[]} loading />);
    expect(container.querySelectorAll(".animate-pulse").length).toBeGreaterThan(0);
  });

  it("renders title, badges, actor, timestamp, and description", () => {
    const items: TimelineEntry[] = [
      {
        id: "1",
        title: "Approved",
        description: "Looks good",
        actor: "Ada Lovelace",
        timestamp: "2024-06-15T10:30:00Z",
        badges: [{ label: "applied", tone: "success" }],
      },
    ];
    render(<Timeline items={items} aria-label="Audit" />);
    expect(screen.getByRole("region", { name: "Audit" })).toBeInTheDocument();
    expect(screen.getByText("Approved")).toBeInTheDocument();
    expect(screen.getByText("Looks good")).toBeInTheDocument();
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
    expect(screen.getByText("applied")).toBeInTheDocument();
    expect(screen.getByText("Approved").closest("li")).toBeTruthy();
    const timeEl = document.querySelector("time");
    expect(timeEl).toHaveAttribute("dateTime", "2024-06-15T10:30:00Z");
  });

  it("groups by day with todayLabel", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2024-06-15T12:00:00Z"));
    const items: TimelineEntry[] = [
      { id: "a", title: "Today event", timestamp: "2024-06-15T08:00:00Z" },
      { id: "b", title: "Yesterday event", timestamp: "2024-06-14T08:00:00Z" },
    ];
    render(<Timeline items={items} groupBy="day" todayLabel="Aujourd'hui" locale="en-GB" />);
    expect(screen.getByText("Aujourd'hui")).toBeInTheDocument();
    expect(screen.getByText("Today event")).toBeInTheDocument();
    expect(screen.getByText("Yesterday event")).toBeInTheDocument();
    vi.useRealTimers();
  });

  it("toggles expandable details", () => {
    const items: TimelineEntry[] = [
      {
        id: "1",
        title: "Change",
        details: <pre>{'{ "ok": true }'}</pre>,
        detailsLabel: "View diff",
      },
    ];
    render(<Timeline items={items} />);
    const btn = screen.getByRole("button", { name: /View diff/i });
    expect(btn).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(btn);
    expect(btn).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(/"ok": true/)).toBeInTheDocument();
  });

  it("invokes linked entity and action slots", () => {
    const onOpen = vi.fn();
    const onAction = vi.fn();
    const items: TimelineEntry[] = [
      {
        id: "1",
        title: "WO linked",
        link: { label: "WO-100", onClick: onOpen },
        action: (
          <button type="button" onClick={onAction}>
            Close
          </button>
        ),
      },
    ];
    render(<Timeline items={items} />);
    fireEvent.click(screen.getByRole("button", { name: "WO-100" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(onAction).toHaveBeenCalledOnce();
  });

  it("applies process step states", () => {
    const items: TimelineEntry[] = [
      { id: "1", title: "Created", stepState: "complete" },
      { id: "2", title: "Pending", stepState: "pending" },
      { id: "3", title: "Current", stepState: "current" },
    ];
    const { container } = render(<Timeline items={items} density="compact" />);
    expect(container.querySelectorAll("li").length).toBe(3);
    expect(screen.getByText("Current")).toBeInTheDocument();
  });

  it("maps domain items via getEntry", () => {
    type Row = { code: string; when: string };
    const rows: Row[] = [{ code: "X-1", when: "2024-01-01T00:00:00Z" }];
    render(
      <Timeline
        items={rows}
        getEntry={(r) => ({
          id: r.code,
          title: r.code,
          timestamp: r.when,
        })}
      />,
    );
    expect(screen.getByText("X-1")).toBeInTheDocument();
  });
});
