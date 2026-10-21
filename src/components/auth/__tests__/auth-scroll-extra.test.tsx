/**
 * Two more screens that must scroll on a short window (html/body/#root are overflow:hidden):
 * the recovery-code dialog and the update install lock overlay.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

const read = (p: string) => readFileSync(resolve(process.cwd(), p), "utf8");

describe("recovery code dialog and update lock scroll", () => {
  it("recovery code dialog is height-capped and scrolls", () => {
    const src = read("src/components/auth/AdminRecoveryCodeDialog.tsx");
    expect(src).toContain("max-h-[calc(100dvh-2rem)]");
    expect(src).toContain("overflow-y-auto");
  });

  it("update install lock overlay scrolls and centres its card with auto margins", () => {
    const src = read("src/components/updates/UpdateInstallLock.tsx");
    expect(src).toContain("fixed inset-0 z-[400] flex overflow-y-auto");
    expect(src).not.toContain("items-center justify-center bg-surface-0/95");
    expect(src).toContain('className="m-auto w-full max-w-md');
  });
});
