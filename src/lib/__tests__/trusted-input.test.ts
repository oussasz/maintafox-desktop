import { describe, expect, it } from "vitest";

import { isTrustedInput } from "@/lib/trusted-input";

describe("isTrustedInput", () => {
  it("rejects an event dispatched by a script", () => {
    let seen: Event | null = null;
    const listener = (e: Event) => {
      seen = e;
    };
    window.addEventListener("pointerdown", listener);
    window.dispatchEvent(new Event("pointerdown"));
    window.removeEventListener("pointerdown", listener);
    expect(seen).not.toBeNull();
    expect(isTrustedInput(seen as unknown as Event)).toBe(false);
  });
});
