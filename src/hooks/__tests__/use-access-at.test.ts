import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const getMyAccessAt = vi.fn<(node: number) => Promise<string[] | null>>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => undefined)),
}));
vi.mock("@/services/rbac-service", () => ({
  getMyAccessAt: (node: number) => getMyAccessAt(node),
}));

import { useAccessAt } from "@/hooks/use-access-at";
import { P } from "@shared/rbac/permissions.generated";

describe("useAccessAt", () => {
  beforeEach(() => getMyAccessAt.mockReset());

  it("answers for the node asked, and never for the previous one while the next loads", async () => {
    let releaseB: (perms: string[]) => void = () => undefined;
    getMyAccessAt.mockImplementation((node) =>
      node === 1
        ? Promise.resolve([P.OT_VIEW])
        : new Promise((resolve) => {
            releaseB = resolve;
          }),
    );
    const { result, rerender } = renderHook(({ node }) => useAccessAt(node), {
      initialProps: { node: 1 as number | null },
    });
    await waitFor(() => expect(result.current.can(P.OT_VIEW)).toBe(true));

    rerender({ node: 2 });
    expect(result.current.can(P.OT_VIEW)).toBe(false);
    releaseB([]);
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.can(P.OT_VIEW)).toBe(false);
  });

  it("treats an administrator as holding every permission, and no node as nothing", async () => {
    getMyAccessAt.mockResolvedValue(null);
    const { result, rerender } = renderHook(({ node }) => useAccessAt(node), {
      initialProps: { node: 5 as number | null },
    });
    await waitFor(() => expect(result.current.can("anything")).toBe(true));
    rerender({ node: null });
    expect(result.current.can("anything")).toBe(false);
  });
});
