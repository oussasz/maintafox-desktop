import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ActivityFeedPanel } from "@/components/activity/ActivityFeedPanel";
import { PermissionProvider } from "@/contexts/PermissionContext";
import { P } from "@shared/rbac/permissions.generated";

const mockGetMyPermissions = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

vi.mock("@/hooks/use-session", () => ({
  useSession: () => ({
    info: {
      is_authenticated: true,
      user_id: 1,
      username: "admin",
    },
    isLoading: false,
  }),
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
  getEventChain: (eventId: number, rootTable: string) =>
    activityMocks.getEventChain(eventId, rootTable),
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
    mockGetMyPermissions.mockResolvedValue([permission(P.LOG_VIEW)]);
    activityMocks.listActivityEvents.mockClear();
    activityMocks.listSavedActivityFilters.mockClear();
  });

  it("exposes readable filter labels (no column names) and applies a saved view", async () => {
    renderPanel();

    await waitFor(() => {
      expect(screen.getByLabelText(/filters\.module|^Module$/)).toBeInTheDocument();
    });
    expect(screen.queryByPlaceholderText("event_class")).not.toBeInTheDocument();
    expect(screen.queryByPlaceholderText("source_module")).not.toBeInTheDocument();
    expect(screen.getByLabelText(/filters\.severity|Severity/)).toBeInTheDocument();
    expect(screen.getByLabelText(/filters\.eventClass|Event type/)).toBeInTheDocument();
    expect(screen.getByLabelText(/filters\.dateFrom|From/)).toBeInTheDocument();
    expect(screen.getByLabelText(/filters\.dateTo|To/)).toBeInTheDocument();
    expect(screen.getByLabelText(/feed\.autoRefresh|Refresh every/)).toBeInTheDocument();

    await waitFor(() => expect(activityMocks.listSavedActivityFilters).toHaveBeenCalled());
    const savedViews = screen.getByLabelText(/feed\.savedViews|Saved views/);
    fireEvent.keyDown(savedViews, { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: "Critical by module" }));

    // The saved filter fills the module filter with its translated label.
    await waitFor(() =>
      expect(screen.getByLabelText(/filters\.module|^Module$/)).toHaveTextContent(/archive/i),
    );
  });

  it("shows a load failure with retry instead of 'no event'", async () => {
    activityMocks.listActivityEvents.mockRejectedValueOnce({
      code: "DATABASE_ERROR",
      message: "down",
    });
    renderPanel();
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText(/feed\.empty|No event for these filters/)).not.toBeInTheDocument();
  });
});
