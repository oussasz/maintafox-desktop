/**
 * Administration and Settings services: argument shaping of the admin commands (user lifecycle
 * with reasons, scoped assignments, ledger filters / CSV export, role editor catalog) and the
 * runtime validation of their payloads, including the defaults kept for older backends.
 */
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  cloneRole,
  deactivateUser,
  getLockoutPolicy,
  setLockoutPolicy,
  getPasswordPolicy,
  setPasswordPolicy,
  exportAdminEventsCsv,
  getRoleEditorCatalog,
  listActiveSessions,
  listAdminEventFacets,
  listAssignableScopeNodes,
  listUsers,
  reactivateUser,
  revokeEmergencyElevation,
  revokeRoleScope,
} from "@/services/rbac-service";
import {
  getSetting,
  getSettingsOverview,
  listSettingChangeEvents,
  readDocumentBrandingLogoPreview,
} from "@/services/settings-service";
import { P } from "@shared/rbac/permissions.generated";

const ipc = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: ipc.invoke,
}));

afterEach(() => {
  ipc.invoke.mockReset();
});

const userRow = {
  id: 7,
  username: "j.doe",
  display_name: "J. Doe",
  personnel_id: null,
  email: null,
  phone: null,
  identity_mode: "local",
  is_active: true,
  force_password_change: false,
  last_seen_at: null,
  locked_until: null,
  roles: [
    {
      assignment_id: 3,
      role_id: 2,
      role_name: "Supervisor",
      scope_type: "org_node",
      scope_reference: "12",
      valid_from: null,
      valid_to: "2026-12-31T23:59:59Z",
      is_emergency: false,
    },
  ],
};

describe("user administration", () => {
  it("sends the list filter and defaults the fields an older backend does not send", async () => {
    ipc.invoke.mockResolvedValueOnce([userRow]);
    const users = await listUsers({ status: "locked", role_id: 2, org_node_id: 12 });
    expect(ipc.invoke).toHaveBeenCalledWith("list_users", {
      filter: { status: "locked", role_id: 2, org_node_id: 12 },
    });
    expect(users[0]?.is_locked).toBe(false);
    expect(users[0]?.last_login_at).toBeNull();
    expect(users[0]?.roles[0]?.scope_label).toBeNull();
    expect(users[0]?.roles[0]?.emergency_expires_at).toBeNull();
  });

  it("rejects a malformed user payload", async () => {
    ipc.invoke.mockResolvedValueOnce([{ ...userRow, is_active: "yes" }]);
    await expect(listUsers({})).rejects.toThrow();
  });

  it("disables and re-enables with the reason in the input", async () => {
    ipc.invoke.mockResolvedValue(undefined);
    await deactivateUser({ user_id: 7, reason: "Left the company" });
    await reactivateUser({ user_id: 7, reason: "Back" });
    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "deactivate_user", {
      input: { user_id: 7, reason: "Left the company" },
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "reactivate_user", {
      input: { user_id: 7, reason: "Back" },
    });
  });

  it("sends an explicit null reason when none is given on role withdrawal", async () => {
    ipc.invoke.mockResolvedValue(undefined);
    await revokeRoleScope(3);
    await revokeRoleScope(3, "Changed team");
    await revokeEmergencyElevation({ assignment_id: 9, reason: null });
    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "revoke_role_scope", {
      assignmentId: 3,
      reason: null,
    });
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "revoke_role_scope", {
      assignmentId: 3,
      reason: "Changed team",
    });
    // The panel reports its own failures: the service does not toast a second time.
    expect(ipc.invoke).toHaveBeenNthCalledWith(
      3,
      "revoke_emergency_elevation",
      { input: { assignment_id: 9, reason: null } },
      { notifyOnError: false },
    );
  });

  it("lists the sessions of one user or of everyone", async () => {
    const session = {
      session_id: "s1",
      user_id: "7",
      username: "j.doe",
      device_id: null,
      device_name: null,
      device_trust_status: "unknown",
      session_started_at: "2026-10-16T08:00:00Z",
      last_activity_at: null,
      is_current_session: false,
      current_role_names: [],
    };
    ipc.invoke.mockResolvedValue([session]);
    await listActiveSessions(7);
    await listActiveSessions();
    expect(ipc.invoke).toHaveBeenNthCalledWith(1, "list_active_sessions", { userId: 7 });
    expect(ipc.invoke).toHaveBeenNthCalledWith(2, "list_active_sessions", { userId: null });
  });

  it("parses the assignable org nodes", async () => {
    ipc.invoke.mockResolvedValueOnce([
      { id: 12, parent_id: null, code: "S1", name: "Site 1", type_label: "Site", depth: 0 },
    ]);
    const nodes = await listAssignableScopeNodes();
    expect(nodes[0]?.name).toBe("Site 1");
    ipc.invoke.mockResolvedValueOnce([{ id: "12" }]);
    await expect(listAssignableScopeNodes()).rejects.toThrow();
  });
});

describe("roles", () => {
  it("clones a role and returns the new id", async () => {
    ipc.invoke.mockResolvedValueOnce({ id: 41 });
    const created = await cloneRole({ source_role_id: 2, name: "Supervisor B", description: null });
    expect(created.id).toBe(41);
    expect(ipc.invoke).toHaveBeenCalledWith("clone_role", {
      input: { source_role_id: 2, name: "Supervisor B", description: null },
    });
  });

  it("parses the role editor catalog and refuses a broken one", async () => {
    const catalog = {
      permissions: [
        {
          id: 1,
          name: P.OT_VIEW,
          description: null,
          category: "ot",
          is_dangerous: false,
          requires_step_up: false,
          is_system: true,
        },
      ],
      dependencies: [
        {
          id: 1,
          permission_name: P.OT_EDIT,
          required_permission_name: P.OT_VIEW,
          dependency_type: "hard",
        },
      ],
      role_holders: [{ role_id: 2, active_users: 4 }],
    };
    ipc.invoke.mockResolvedValueOnce(catalog);
    await expect(getRoleEditorCatalog()).resolves.toEqual(catalog);
    ipc.invoke.mockResolvedValueOnce({ permissions: [], dependencies: [] });
    await expect(getRoleEditorCatalog()).rejects.toThrow();
  });
});

describe("admin ledger", () => {
  it("parses the filter choices", async () => {
    ipc.invoke.mockResolvedValueOnce({
      actions: ["user_created"],
      actors: [{ id: 1, label: "admin", count: 3 }],
      target_users: [],
      target_roles: [],
      apply_results: ["applied", "blocked"],
    });
    const facets = await listAdminEventFacets();
    expect(facets.actors[0]?.count).toBe(3);
    ipc.invoke.mockResolvedValueOnce({ actions: "user_created" });
    await expect(listAdminEventFacets()).rejects.toThrow();
  });

  it("exports the filtered ledger as CSV text", async () => {
    ipc.invoke.mockResolvedValueOnce("id,acted_at\n1,2026-10-16T08:00:00Z\n");
    const csv = await exportAdminEventsCsv({ apply_result: "blocked", date_from: "2026-10-01" });
    expect(csv.startsWith("id,acted_at")).toBe(true);
    expect(ipc.invoke).toHaveBeenCalledWith("export_admin_events_csv", {
      filter: { apply_result: "blocked", date_from: "2026-10-01" },
    });
    ipc.invoke.mockResolvedValueOnce(42);
    await expect(exportAdminEventsCsv({})).rejects.toThrow();
  });
});

describe("settings", () => {
  const definition = {
    key: "wo.reopen_window_days",
    category: "work_orders",
    scope: "tenant",
    kind: "integer",
    options: [],
    min: 0,
    max: 3650,
    shipped_value_json: "30",
    current_value_json: "45",
    differs_from_shipped: true,
    editor: "section",
    risk: "unexpected",
    required_permission: P.ADM_SETTINGS,
    locked_reason: null,
    last_modified_at: null,
    last_modified_by: null,
  };

  it("parses the overview and treats an unknown risk as low", async () => {
    ipc.invoke.mockResolvedValueOnce({
      definitions: [definition, { ...definition, key: "x", risk: "high" }],
      roles_allowed_to_change: ["Administrator"],
    });
    const overview = await getSettingsOverview();
    expect(overview.definitions[0]?.risk).toBe("low");
    expect(overview.definitions[1]?.risk).toBe("high");
    expect(overview.roles_allowed_to_change).toEqual(["Administrator"]);
  });

  it("refuses a definition of an unknown kind", async () => {
    ipc.invoke.mockResolvedValueOnce({
      definitions: [{ ...definition, kind: "freeform" }],
      roles_allowed_to_change: [],
    });
    await expect(getSettingsOverview()).rejects.toThrow();
  });

  it("filters the change history by key and defaults missing values", async () => {
    ipc.invoke.mockResolvedValueOnce([
      {
        id: 1,
        setting_key_or_domain: "wo.reopen_window_days",
        change_summary: "x",
        old_value_hash: null,
        new_value_hash: "h",
        changed_by_id: null,
        changed_at: "2026-10-16T08:00:00Z",
        required_step_up: false,
        apply_result: "applied",
      },
    ]);
    const events = await listSettingChangeEvents(20, "wo.reopen_window_days");
    expect(ipc.invoke).toHaveBeenCalledWith("list_setting_change_events", {
      limit: 20,
      settingKey: "wo.reopen_window_days",
    });
    expect(events[0]?.old_value_json).toBeNull();
    expect(events[0]?.changed_by_name).toBeNull();
  });

  it("returns null when a setting or the header logo is absent", async () => {
    ipc.invoke.mockResolvedValueOnce(null);
    await expect(getSetting("wo.reopen_window_days", "tenant")).resolves.toBeNull();
    ipc.invoke.mockResolvedValueOnce(null);
    await expect(readDocumentBrandingLogoPreview()).resolves.toBeNull();
  });

  it("parses a stored setting, coercing unknown risk and status values", async () => {
    const row = {
      id: 1,
      setting_key: "wo.reopen_window_days",
      setting_scope: "tenant",
      setting_value_json: "30",
      category: "work_orders",
      setting_risk: "high",
      validation_status: "legacy",
      secret_ref_id: null,
      last_modified_by_id: null,
      last_modified_at: "2026-10-16T08:00:00Z",
    };
    ipc.invoke.mockResolvedValueOnce(row);
    const parsed = await getSetting("wo.reopen_window_days");
    expect(parsed?.setting_risk).toBe("high");
    expect(parsed?.validation_status).toBe("valid");
    ipc.invoke.mockResolvedValueOnce({
      ...row,
      setting_risk: "medium",
      validation_status: "draft",
    });
    const draft = await getSetting("wo.reopen_window_days");
    expect(draft?.setting_risk).toBe("low");
    expect(draft?.validation_status).toBe("draft");
  });

  it("parses the header logo preview", async () => {
    ipc.invoke.mockResolvedValueOnce({
      mime_type: "image/png",
      data_base64: "AAAA",
      relative_path: "branding/logo.png",
    });
    await expect(readDocumentBrandingLogoPreview()).resolves.toMatchObject({
      mime_type: "image/png",
    });
  });
});

describe("security policies", () => {
  it("saves the password policy as one validated change", async () => {
    ipc.invoke.mockResolvedValueOnce(undefined);
    const policy = {
      max_age_days: 90,
      warn_days_before_expiry: 14,
      min_length: 12,
      require_uppercase: true,
      require_lowercase: true,
      require_digit: true,
      require_special: false,
      unreadable_keys: [],
    };
    await setPasswordPolicy(policy);
    expect(ipc.invoke).toHaveBeenCalledWith(
      "set_password_policy",
      { input: policy },
      { notifyOnError: false },
    );
  });

  it("reads which stored password rules could not be read", async () => {
    const stored = {
      max_age_days: 90,
      warn_days_before_expiry: 14,
      min_length: 8,
      require_uppercase: true,
      require_lowercase: true,
      require_digit: true,
      require_special: false,
      unreadable_keys: ["password_min_length"],
    };
    ipc.invoke.mockResolvedValueOnce(stored);
    await expect(getPasswordPolicy()).resolves.toEqual(stored);
    ipc.invoke.mockResolvedValueOnce({ ...stored, unreadable_keys: undefined });
    await expect(getPasswordPolicy()).rejects.toThrow();
  });

  it("reads and saves the sign-in lockout", async () => {
    ipc.invoke.mockResolvedValueOnce({ max_attempts: 5, lockout_minutes: 15, progressive: true });
    await expect(getLockoutPolicy()).resolves.toEqual({
      max_attempts: 5,
      lockout_minutes: 15,
      progressive: true,
    });
    ipc.invoke.mockResolvedValueOnce({ max_attempts: "5" });
    await expect(getLockoutPolicy()).rejects.toThrow();
    ipc.invoke.mockResolvedValueOnce(undefined);
    await setLockoutPolicy({ max_attempts: 6, lockout_minutes: 30, progressive: false });
    expect(ipc.invoke).toHaveBeenLastCalledWith("set_lockout_policy", {
      input: { max_attempts: 6, lockout_minutes: 30, progressive: false },
    });
  });
});
