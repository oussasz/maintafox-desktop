import { describe, expect, it } from "vitest";

import { historyLinkTarget } from "@/components/assets/history/history-links";
import type { AssetHistoryEvent } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function ev(ref: Partial<NonNullable<AssetHistoryEvent["ref"]>> | null): AssetHistoryEvent {
  return { id: "1", ref } as unknown as AssetHistoryEvent;
}

describe("historyLinkTarget", () => {
  it("sends a linked document to the documentation module and needs its view right", () => {
    expect(historyLinkTarget(ev({ entity_type: "document", route_hint: "/documents" }))).toEqual({
      path: "/documentation",
      permission: P.DOC_VIEW,
    });
  });

  it("needs the work-order or request view right for those events", () => {
    expect(
      historyLinkTarget(ev({ entity_type: "wo", route_hint: "/work-orders?openWo=5" })).permission,
    ).toBe(P.OT_VIEW);
    expect(historyLinkTarget(ev({ entity_type: "di", entity_id: 9 }))).toEqual({
      path: "/requests?openDi=9",
      permission: P.DI_VIEW,
    });
  });

  it("has no target without a reference", () => {
    expect(historyLinkTarget(ev(null))).toEqual({ path: null, permission: null });
  });
});
