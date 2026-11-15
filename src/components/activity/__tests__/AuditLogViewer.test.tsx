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
  getAuditEvent: vi.fn(
    (_id?: number): Promise<unknown> => Promise.reject(new Error("detail unavailable")),
  ),
  exportAuditLog: vi.fn((_payload?: unknown) => Promise.resolve()),
  listAuditFilterOptions: vi.fn(() =>
    Promise.resolve({
      actions: ["auth.login.success", "wo.closed"],
      actors: [{ id: 1, label: "Djamel Ferhat" }],
    }),
  ),
}));

vi.mock("@/services/activity-service", () => ({
  listAuditEvents: (filter: unknown) => auditMocks.listAuditEvents(filter),
  getAuditEvent: (id: number) => auditMocks.getAuditEvent(id),
  exportAuditLog: (payload: unknown) => auditMocks.exportAuditLog(payload),
  listAuditFilterOptions: () => auditMocks.listAuditFilterOptions(),
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
    expect(screen.getByLabelText(/audit\.filters\.user$|^User$/)).toBeInTheDocument();
    expect(
      screen.getByLabelText(/audit\.filters\.objectSearch|Work order, equipment or permit number/),
    ).toBeInTheDocument();
    // No box to type an internal user number or a raw action code any more.
    expect(screen.queryByLabelText(/userNumber|User number/)).not.toBeInTheDocument();
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
          target_type: "session",
          target_id: `sess-${i + 1}`,
          actor_id: 1,
          actor_username: "admin",
          auth_context: "password",
          result: "success",
          happened_at: "2026-04-13T11:00:00.000Z",
          retention_class: "standard",
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

  it("searches by OT number and shows the object by its code and the changed fields", async () => {
    auditMocks.listAuditEvents.mockImplementation((_filter?: unknown) =>
      Promise.resolve([
        {
          id: 562,
          action_code: "wo.closed",
          target_type: "work_order",
          target_id: "12",
          target_label: "OT-2026-0042 — Fuite pompe P-101",
          actor_id: 1,
          actor_username: "d.ferhat",
          actor_display_name: "Djamel Ferhat",
          auth_context: "system",
          result: "success",
          happened_at: "2026-10-28T18:26:00Z",
          retention_class: "standard",
        },
      ]),
    );
    auditMocks.getAuditEvent.mockImplementation((_id?: number) =>
      Promise.resolve({
        id: 562,
        action_code: "wo.closed",
        target_type: "work_order",
        target_id: "12",
        actor_id: 1,
        actor_username: "d.ferhat",
        auth_context: "system",
        result: "success",
        before_hash: null,
        after_hash: null,
        happened_at: "2026-10-28T18:26:00Z",
        retention_class: "standard",
        details_json: {
          summary: "Work order closed",
          detail: { changes: [{ field: "closed_at", old: null, new: "2026-10-28T18:26:00Z" }] },
        },
        from_other_device: false,
      }),
    );
    renderViewer();
    await waitFor(() => expect(auditMocks.listAuditFilterOptions).toHaveBeenCalled());

    fireEvent.change(
      screen.getByLabelText(/audit\.filters\.objectSearch|Work order, equipment or permit number/),
      { target: { value: "OT-2026-0042" } },
    );
    fireEvent.click(screen.getByRole("button", { name: /filters\.apply|Apply/ }));
    await waitFor(() =>
      expect(auditMocks.listAuditEvents).toHaveBeenLastCalledWith(
        expect.objectContaining({ object_search: "OT-2026-0042", offset: 0 }),
      ),
    );

    expect(await screen.findByText("OT-2026-0042 — Fuite pompe P-101")).toBeInTheDocument();
    expect(screen.getByText("Djamel Ferhat")).toBeInTheDocument();
    expect(screen.queryByText("work order · 12")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /OT-2026-0042/ }));
    const list = await screen.findByTestId("audit-change-list");
    expect(list.textContent).toMatch(/closed at|Closing date|Date de clôture/);
    expect(list.textContent).toContain("→");
  });
});
