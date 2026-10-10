import { describe, expect, it } from "vitest";

import { activationRequiresLocalDataReset } from "@/services/product-license-service";

describe("activationRequiresLocalDataReset", () => {
  it("keeps local data when re-activating the tenant already bound on this device", () => {
    expect(activationRequiresLocalDataReset("tenant_9b38", "tenant_9b38")).toBe(false);
    expect(activationRequiresLocalDataReset(" tenant_9b38 ", "tenant_9b38")).toBe(false);
  });

  it("wipes local data on a first activation", () => {
    expect(activationRequiresLocalDataReset(null, "tenant_9b38")).toBe(true);
    expect(activationRequiresLocalDataReset(undefined, "tenant_9b38")).toBe(true);
    expect(activationRequiresLocalDataReset("  ", "tenant_9b38")).toBe(true);
  });

  it("wipes local data when switching to another tenant", () => {
    expect(activationRequiresLocalDataReset("tenant_old", "tenant_new")).toBe(true);
  });
});
