import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ActivityFeedPanel } from "@/components/activity/ActivityFeedPanel";
import { PermissionProvider } from "@/contexts/PermissionContext";

const mockGetMyPermissions = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

const activityMocks = vi.hoisted(() => ({
  listActivityEvents: vi.fn((_filter?: unknown) => Promise.resolve([])),
  listSavedActivityFilters: vi.fn(() =>
    Promise.resolve([
      {
        id: 11,
        user_id: 1,
        view_name: "Critical by module",
        filter_json: { event_class: "ops", source_module: "archive", severity: "critical" },
        is_default: false,
      },
    ]),
  ),
  saveActivityFilter: vi.fn((_payload?: unknown) => Promise.resolve()),
  getEventChain: vi.fn((_eventId?: number, _rootTable?: string) => Promise.resolve({ events: [] })),
}));

vi.mock("@/services/activity-service", () => ({
  listActivityEvents: (filter: unknown) => activityMocks.listActivityEvents(filter),
  listSavedActivityFilters: () => activityMocks.listSavedActivityFilters(),
  saveActivityFilter: (payload: unknown) => activityMocks.saveActivityFilter(payload),
  getEventChain: (eventId: number, rootTable: string) => activityMocks.getEventChain(eventId, rootTable),
}));

function renderPanel() {
  return render(
    <PermissionProvider>
      <ActivityFeedPanel />
    </PermissionProvider>,
  );
}

function permission(name: string) {
  return {
    name,
    description: "",
    category: "activity",
    is_dangerous: false,
    requires_step_up: false,
  };
}

describe("ActivityFeedPanel accessibility and saved views", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
    mockGetMyPermissions.mockResolvedValue([permission("log.view")]);
    activityMocks.listActivityEvents.mockClear();
    activityMocks.listSavedActivityFilters.mockClear();
  });

  it("exposes labeled filter controls and keeps selected saved view value", async () => {
    renderPanel();

    await waitFor(() => {
      expect(activityMocks.listSavedActivityFilters).toHaveBeenCalled();
    });

    expect(screen.getByLabelText("Event class filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Source module filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Severity filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Entity scope ID filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Correlation ID filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Date from filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Date to filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Auto-refresh activity feed")).toBeInTheDocument();

    const savedViewSelect = screen.getByLabelText("Saved activity views") as HTMLSelectElement;
    fireEvent.change(savedViewSelect, { target: { value: "11" } });
    expect(savedViewSelect.value).toBe("11");
  });
});
