import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AuditLogViewer } from "@/components/activity/AuditLogViewer";
import { PermissionProvider } from "@/contexts/PermissionContext";
import { P } from "@shared/rbac/permissions.generated";

const mockGetMyPermissions = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

const auditMocks = vi.hoisted(() => ({
  listAuditEvents: vi.fn((_filter?: unknown) =>
    Promise.resolve([
      {
        id: 77,
        action_code: "auth.login.success",
        target_type: "session",
        target_id: "sess-1",
        actor_id: 1,
        actor_username: "admin",
        auth_context: "password",
        result: "success",
        happened_at: "2026-04-13T11:00:00.000Z",
        retention_class: "security",
      },
    ]),
  ),
  getAuditEvent: vi.fn((_id?: number) => Promise.reject(new Error("detail unavailable"))),
  exportAuditLog: vi.fn((_payload?: unknown) => Promise.resolve()),
}));

vi.mock("@/services/activity-service", () => ({
  listAuditEvents: (filter: unknown) => auditMocks.listAuditEvents(filter),
  getAuditEvent: (id: number) => auditMocks.getAuditEvent(id),
  exportAuditLog: (payload: unknown) => auditMocks.exportAuditLog(payload),
}));

vi.mock("@/services/auth-service", () => ({
  getSessionInfo: vi.fn(() => Promise.resolve({ user_id: 1 })),
}));

function renderViewer() {
  return render(
    <PermissionProvider>
      <AuditLogViewer />
    </PermissionProvider>,
  );
}

function permission(name: string) {
  return {
    name,
    description: "",
    category: "audit",
    is_dangerous: false,
    requires_step_up: false,
  };
}

describe("AuditLogViewer labels and detail errors", () => {
  beforeEach(() => {
    mockGetMyPermissions.mockReset();
    mockGetMyPermissions.mockResolvedValue([permission(P.LOG_VIEW), permission(P.LOG_EXPORT)]);
    auditMocks.listAuditEvents.mockClear();
    auditMocks.getAuditEvent.mockClear();
  });

  it("renders labeled filter inputs and surfaces detail fetch failures", async () => {
    renderViewer();

    await waitFor(() => {
      expect(auditMocks.listAuditEvents).toHaveBeenCalled();
    });

    expect(screen.getByLabelText(/audit\.filters\.action|^Action$/)).toBeInTheDocument();
    expect(screen.getByLabelText(/audit\.filters\.result|^Result$/)).toBeInTheDocument();
    expect(screen.getByLabelText(/audit\.filters\.userNumber|User number/)).toBeInTheDocument();
    expect(screen.getByLabelText(/audit\.filters\.retention|^Retention$/)).toBeInTheDocument();
    expect(screen.getByLabelText(/filters\.dateFrom|From/)).toBeInTheDocument();
    expect(screen.getByLabelText(/filters\.dateTo|To/)).toBeInTheDocument();

    fireEvent.click(await screen.findByRole("button", { name: /auth login success/i }));

    await waitFor(() => {
      expect(auditMocks.getAuditEvent).toHaveBeenCalledWith(77);
      expect(screen.getByText("detail unavailable")).toBeInTheDocument();
    });
  });

  it("reloads when moving to the next page", async () => {
    auditMocks.listAuditEvents.mockImplementation((_filter?: unknown) =>
      Promise.resolve(
        Array.from({ length: 40 }, (_, i) => ({
          id: i + 1,
          action_code: "auth.login.success",
          target_type: null,
          target_id: null,
          actor_id: 1,
          actor_username: "admin",
          auth_context: "password",
          result: "success",
          happened_at: "2026-04-13T11:00:00.000Z",
          retention_class: "standard",
          from_other_device: false,
        })),
      ),
    );
    renderViewer();
    await waitFor(() => expect(auditMocks.listAuditEvents).toHaveBeenCalledTimes(1));
    fireEvent.click(await screen.findByRole("button", { name: /pager\.next|Next/ }));
    await waitFor(() =>
      expect(auditMocks.listAuditEvents).toHaveBeenLastCalledWith(
        expect.objectContaining({ offset: 40 }),
      ),
    );
  });
});
