import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AuditLogViewer } from "@/components/activity/AuditLogViewer";
import { PermissionProvider } from "@/contexts/PermissionContext";

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
    mockGetMyPermissions.mockResolvedValue([permission("log.view"), permission("log.export")]);
    auditMocks.listAuditEvents.mockClear();
    auditMocks.getAuditEvent.mockClear();
  });

  it("renders labeled filter inputs and surfaces detail fetch failures", async () => {
    renderViewer();

    await waitFor(() => {
      expect(auditMocks.listAuditEvents).toHaveBeenCalled();
    });

    expect(screen.getByLabelText("Action code filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Result filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Actor ID filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Retention class filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Date from filter")).toBeInTheDocument();
    expect(screen.getByLabelText("Date to filter")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /auth\.login\.success/i }));

    await waitFor(() => {
      expect(auditMocks.getAuditEvent).toHaveBeenCalledWith(77);
      expect(screen.getByText("detail unavailable")).toBeInTheDocument();
    });
  });
});
