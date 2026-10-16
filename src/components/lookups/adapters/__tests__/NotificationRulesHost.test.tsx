import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationRulesHost } from "@/components/lookups/adapters/NotificationRulesHost";
import type { NotificationConfig } from "@/services/notification-service";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, o?: Record<string, unknown>) => {
      const vals = Object.entries(o ?? {})
        .filter(([k]) => k !== "defaultValue")
        .map(([k, v]) => `${k}=${String(v)}`);
      return vals.length ? `${key}[${vals.join(",")}]` : key;
    },
    i18n: { language: "en" },
  }),
}));

const perms = vi.hoisted(() => ({ canManage: true }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: () => perms.canManage, isLoading: false }),
}));

const svc = vi.hoisted(() => ({
  getNotificationConfig: vi.fn(),
  updateNotificationRule: vi.fn(),
  updateNotificationEscalationStep: vi.fn(),
  addNotificationEscalationStep: vi.fn(),
  removeNotificationEscalationStep: vi.fn(),
  setNotificationRetention: vi.fn(),
}));
vi.mock("@/services/notification-service", () => svc);

const config: NotificationConfig = {
  rules: [
    {
      rule_id: 7,
      category_code: "wo_overdue",
      module_code: "ot",
      default_severity: "warning",
      is_user_configurable: true,
      fixed_routing: false,
      is_active: true,
      audiences: ["assignee", "permission:ot.approve"],
      requires_ack: true,
      escalation_policy_id: 1,
      uses_lead_time: false,
      lead_time_minutes: null,
    },
  ],
  policies: [
    {
      id: 1,
      name: "Default Critical Escalation",
      steps: [
        { id: 11, level_no: 1, wait_minutes: 30, target: "supervisor", reference: null },
        { id: 12, level_no: 2, wait_minutes: 60, target: "entity_manager", reference: null },
      ],
    },
  ],
  retention: { expiry_days: 180, is_default: true, default_days: 180, min_days: 7, max_days: 3650 },
  audiences: ["assignee", "planner", "team"],
  escalation_targets: ["supervisor", "planner", "entity_manager", "permission", "role"],
};

beforeEach(() => {
  vi.clearAllMocks();
  perms.canManage = true;
  svc.getNotificationConfig.mockResolvedValue(config);
  svc.updateNotificationRule.mockResolvedValue(config);
  svc.updateNotificationEscalationStep.mockResolvedValue(config);
  svc.setNotificationRetention.mockResolvedValue(config);
});

describe("NotificationRulesHost", () => {
  it("shows the categories with their recipients and the shipped escalation and retention", async () => {
    render(<NotificationRulesHost />);
    expect(await screen.findByText("category.wo_overdue")).toBeInTheDocument();
    expect(screen.getByText("audience.assignee")).toBeInTheDocument();
    expect(screen.getByText("audience.permission[permission=ot.approve]")).toBeInTheDocument();
    expect(screen.getByDisplayValue("30")).toBeInTheDocument();
    expect(screen.getByDisplayValue("180")).toBeInTheDocument();
    expect(screen.getByText("rules.defaultValue")).toBeInTheDocument();
  });

  it("removes a recipient through the backend", async () => {
    render(<NotificationRulesHost />);
    fireEvent.click(
      await screen.findByRole("button", {
        name: "rules.removeAudience[audience=audience.assignee]",
      }),
    );
    await waitFor(() =>
      expect(svc.updateNotificationRule).toHaveBeenCalledWith({
        rule_id: 7,
        audiences: ["permission:ot.approve"],
      }),
    );
  });

  it("saves an escalation delay only when it changed", async () => {
    render(<NotificationRulesHost />);
    const delay = await screen.findByDisplayValue("30");
    expect(screen.queryAllByRole("button", { name: "rules.save" })).toHaveLength(0);
    fireEvent.change(delay, { target: { value: "45" } });
    fireEvent.click(screen.getByRole("button", { name: "rules.save" }));
    await waitFor(() =>
      expect(svc.updateNotificationEscalationStep).toHaveBeenCalledWith(11, {
        wait_minutes: 45,
        target: "supervisor",
        reference: null,
      }),
    );
  });

  it("is read only without ref.manage", async () => {
    perms.canManage = false;
    render(<NotificationRulesHost />);
    expect(await screen.findByText("rules.readOnly")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /rules\.removeAudience/ })).toBeNull();
  });
});
