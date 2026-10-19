import { renderHook, waitFor } from "@testing-library/react";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const getMyAccessAt = vi.fn<(node: number) => Promise<string[] | null>>();
// The `rbac-changed` handlers the hook registered, so a test can announce a role change.
const rbacChanged = vi.hoisted(() => [] as (() => void)[]);

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((_event: string, handler: () => void) => {
    rbacChanged.push(handler);
    return Promise.resolve(() => undefined);
  }),
}));
vi.mock("@/services/rbac-service", () => ({
  getMyAccessAt: (node: number) => getMyAccessAt(node),
}));

import { useAccessAt, useAccessAtNodes } from "@/hooks/use-access-at";
import { P } from "@shared/rbac/permissions.generated";

describe("useAccessAt", () => {
  // Braces matter: vitest runs a function returned from beforeEach as the test's teardown, and
  // `mockReset()` returns the mock, so the teardown would await test 1's never-settling promise.
  beforeEach(() => {
    getMyAccessAt.mockReset();
  });

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

  it("never renders the previous node's rights, not even for one frame", async () => {
    getMyAccessAt.mockImplementation((node) =>
      node === 1 ? Promise.resolve([P.OT_VIEW]) : new Promise(() => undefined),
    );
    const frames: { node: number | null; can: boolean; loading: boolean }[] = [];
    const { result, rerender } = renderHook(
      ({ node }) => {
        const access = useAccessAt(node);
        frames.push({ node, can: access.can(P.OT_VIEW), loading: access.loading });
        return access;
      },
      { initialProps: { node: 1 as number | null } },
    );
    await waitFor(() => expect(result.current.can(P.OT_VIEW)).toBe(true));
    const from = frames.length;
    rerender({ node: 2 });
    const after = frames.slice(from);
    expect(after.length).toBeGreaterThan(0);
    for (const f of after) {
      expect(f).toEqual({ node: 2, can: false, loading: true });
    }
  });

  it("ignores a late answer for a node left and then returned to", async () => {
    const pending: ((perms: string[]) => void)[] = [];
    getMyAccessAt.mockImplementation(
      () =>
        new Promise((resolve) => {
          pending.push(resolve);
        }),
    );
    const { result, rerender } = renderHook(({ node }) => useAccessAt(node), {
      initialProps: { node: 1 as number | null },
    });
    rerender({ node: 2 });
    rerender({ node: 1 });
    await waitFor(() => expect(pending).toHaveLength(3));
    // The first request for node 1 (left before it answered) grants; it must not count.
    await act(async () => pending[0]?.([P.OT_VIEW]));
    expect(result.current.can(P.OT_VIEW)).toBe(false);
    expect(result.current.loading).toBe(true);
    await act(async () => pending[2]?.([]));
    expect(result.current.loading).toBe(false);
    expect(result.current.can(P.OT_VIEW)).toBe(false);
  });

  it("drops the answer at once when roles change, until the new one arrives", async () => {
    rbacChanged.length = 0;
    let release: (perms: string[]) => void = () => undefined;
    getMyAccessAt
      .mockResolvedValueOnce([P.OT_VIEW])
      .mockImplementationOnce(() => new Promise((resolve) => (release = resolve)));
    const { result } = renderHook(() => useAccessAt(7));
    await waitFor(() => expect(result.current.can(P.OT_VIEW)).toBe(true));
    await waitFor(() => expect(rbacChanged.length).toBeGreaterThan(0));
    act(() => rbacChanged.forEach((h) => h()));
    expect(result.current.can(P.OT_VIEW)).toBe(false);
    expect(result.current.loading).toBe(true);
    await act(async () => release([]));
    expect(result.current.loading).toBe(false);
    expect(result.current.can(P.OT_VIEW)).toBe(false);
  });

  it("stops loading when the node is cleared while a load is pending", async () => {
    getMyAccessAt.mockImplementation(() => new Promise(() => undefined));
    const { result, rerender } = renderHook(({ node }) => useAccessAt(node), {
      initialProps: { node: 3 as number | null },
    });
    await waitFor(() => expect(result.current.loading).toBe(true));
    rerender({ node: null });
    expect(result.current.loading).toBe(false);
    expect(result.current.can("anything")).toBe(false);
  });
});

describe("useAccessAtNodes", () => {
  beforeEach(() => {
    getMyAccessAt.mockReset();
  });

  it("allows only when one of the rights is held at EVERY node", async () => {
    getMyAccessAt.mockImplementation((node) =>
      Promise.resolve(node === 1 ? [P.DI_SCREEN] : node === 2 ? [P.DI_REVIEW] : []),
    );
    const { result, rerender } = renderHook(({ nodes }) => useAccessAtNodes(nodes), {
      initialProps: { nodes: [1, 2] },
    });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.canAllAny([P.DI_SCREEN, P.DI_REVIEW])).toBe(true);
    expect(result.current.canAllAny([P.DI_SCREEN])).toBe(false);

    // A third node where neither right is held (e.g. plant-wide only, node 0) refuses the lot.
    rerender({ nodes: [1, 2, 0] });
    expect(result.current.canAllAny([P.DI_SCREEN, P.DI_REVIEW])).toBe(false);
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.canAllAny([P.DI_SCREEN, P.DI_REVIEW])).toBe(false);
  });

  it("treats an administrator as holding every right, and no node as nothing to refuse", async () => {
    getMyAccessAt.mockResolvedValue(null);
    const { result, rerender } = renderHook(({ nodes }) => useAccessAtNodes(nodes), {
      initialProps: { nodes: [7] },
    });
    await waitFor(() => expect(result.current.canAllAny(["anything"])).toBe(true));
    rerender({ nodes: [] });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.canAllAny(["anything"])).toBe(true);
  });
});
