import { describe, expect, it } from "vitest";

import {
  capabilityMapForClaimEdition,
  capabilityMapForEdition,
  parseEditionId,
} from "@/lib/edition-capability-catalog";

describe("edition-capability-catalog fallback", () => {
  it("parses only system edition ids", () => {
    expect(parseEditionId("professional")).toBe("professional");
    expect(parseEditionId("pilot-v1")).toBeNull();
    expect(parseEditionId("unknown")).toBeNull();
  });

  it("uses Core map for custom or unknown claim editions", () => {
    const core = capabilityMapForEdition("core");
    expect(capabilityMapForClaimEdition("pilot-v1")).toEqual(core);
    expect(capabilityMapForClaimEdition("enterprise")["planning"]).toBe(true);
    expect(capabilityMapForClaimEdition("enterprise")["finance"]).toBe(true);
    expect(capabilityMapForClaimEdition("pilot-v1")["finance"]).toBe(false);
  });

  it("enables training only on Enterprise and Development factory maps", () => {
    expect(capabilityMapForEdition("core")["training"]).toBe(false);
    expect(capabilityMapForEdition("professional")["training"]).toBe(false);
    expect(capabilityMapForEdition("enterprise")["training"]).toBe(true);
    expect(capabilityMapForEdition("development")["training"]).toBe(true);
  });
});
