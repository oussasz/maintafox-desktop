import { describe, expect, it } from "vitest";

import { describeAdminEvent } from "@/components/admin/admin-event-text";
import type { AdminChangeEventDetail } from "@shared/ipc-types";

// The test translator returns the key and its values, so the assertions read what the screen asks for.
const t = (key: string, o?: Record<string, unknown>) => (o ? `${key}${JSON.stringify(o)}` : key);

function evt(over: Partial<AdminChangeEventDetail>): AdminChangeEventDetail {
  return {
    id: 1,
    action: "user_created",
    actor_id: 1,
    actor_username: "admin",
    target_user_id: 2,
    target_username: "jdoe",
    target_role_id: null,
    target_role_name: null,
    acted_at: "2026-10-22T08:00:00Z",
    scope_type: null,
    scope_reference: null,
    summary: "User account 'jdoe' created",
    diff_json: null,
    step_up_used: false,
    apply_result: "applied",
    from_other_device: false,
    ...over,
  };
}

describe("describeAdminEvent", () => {
  it("writes the sentence from the action, not from the stored English summary", () => {
    expect(describeAdminEvent(evt({}), "jdoe", t)).toBe(
      'console.summary.user_created{"target":"jdoe"}',
    );
  });

  it("counts the permissions of a role change from its diff", () => {
    const text = describeAdminEvent(
      evt({
        action: "role_updated",
        diff_json: JSON.stringify({
          permissions_added: ["a", "b", "c"],
          permissions_removed: ["d"],
        }),
      }),
      "Planner",
      t,
    );
    expect(text).toBe('console.summary.role_updated{"target":"Planner","added":3,"removed":1}');
  });

  it("never shows the backend's own sentence for a refusal", () => {
    const refused = evt({ apply_result: "blocked", summary: "Permission requise : adm.users" });
    expect(describeAdminEvent(refused, null, t)).toBe("console.refusal.generic");
    const coded = evt({ apply_result: "blocked", summary: "permission_denied" });
    expect(describeAdminEvent(coded, null, t)).toBe("console.refusal.permission_denied");
  });

  it("shows the reason a person typed for a deactivation", () => {
    const text = describeAdminEvent(
      evt({ action: "user_deactivated", summary: "Left the company" }),
      "jdoe",
      t,
    );
    expect(text).toBe('console.reasonLine{"reason":"Left the company"}');
  });
});
