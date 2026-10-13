/**
 * work-order-priorities-catalog-store — behaviour tests.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { listWorkOrderPriorities } from "@/services/wo-service";
import type { WorkOrderPriorityOption } from "@shared/ipc-types";

import {
  refreshWorkOrderPrioritiesCatalog,
  useWorkOrderPrioritiesCatalog,
} from "../work-order-priorities-catalog-store";

vi.mock("@/services/wo-service", () => ({
  listWorkOrderPriorities: vi.fn(),
}));

const mockList = vi.mocked(listWorkOrderPriorities);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makePriority(overrides: Partial<WorkOrderPriorityOption> = {}): WorkOrderPriorityOption {
  return {
    id: 1,
    level: 1,
    code: "P1",
    label: "Critical",
    label_fr: "Critique",
    hex_color: "#ff0000",
    is_system: true,
    is_active: true,
    ...overrides,
  };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const initial = useWorkOrderPrioritiesCatalog.getState();

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockReset();
  useWorkOrderPrioritiesCatalog.setState(initial, true);
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("work-order-priorities-catalog-store", () => {
  it("starts empty and idle", () => {
    const s = useWorkOrderPrioritiesCatalog.getState();
    expect(s.priorities).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });

  it("load stores the priorities returned by the service", async () => {
    const rows = [makePriority(), makePriority({ id: 2, level: 2, code: "P2", label: "High" })];
    mockList.mockResolvedValueOnce(rows);

    await useWorkOrderPrioritiesCatalog.getState().load();

    expect(mockList).toHaveBeenCalledTimes(1);
    expect(mockList).toHaveBeenCalledWith();
    const s = useWorkOrderPrioritiesCatalog.getState();
    expect(s.priorities).toEqual(rows);
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });

  it("sets loading while in flight and clears a stale error", async () => {
    const d = deferred<WorkOrderPriorityOption[]>();
    mockList.mockReturnValueOnce(d.promise);
    useWorkOrderPrioritiesCatalog.setState({ error: "stale" });

    const p = useWorkOrderPrioritiesCatalog.getState().load();
    expect(useWorkOrderPrioritiesCatalog.getState().loading).toBe(true);
    expect(useWorkOrderPrioritiesCatalog.getState().error).toBeNull();

    d.resolve([makePriority()]);
    await p;
    expect(useWorkOrderPrioritiesCatalog.getState().loading).toBe(false);
  });

  it("on failure sets error, clears loading, keeps old priorities and does not rethrow", async () => {
    const previous = [makePriority()];
    useWorkOrderPrioritiesCatalog.setState({ priorities: previous });
    mockList.mockRejectedValueOnce({ code: "PERMISSION_DENIED", message: "nope" });

    await expect(useWorkOrderPrioritiesCatalog.getState().load()).resolves.toBeUndefined();

    const s = useWorkOrderPrioritiesCatalog.getState();
    expect(s.error).toBe("nope");
    expect(s.loading).toBe(false);
    expect(s.priorities).toBe(previous);
  });

  it("uses a plain string rejection as the error message", async () => {
    mockList.mockRejectedValueOnce("plain failure");
    await useWorkOrderPrioritiesCatalog.getState().load();
    expect(useWorkOrderPrioritiesCatalog.getState().error).toBe("plain failure");
  });

  it("refreshWorkOrderPrioritiesCatalog delegates to the store load action", async () => {
    const rows = [makePriority({ id: 9, code: "P9" })];
    mockList.mockResolvedValueOnce(rows);

    await refreshWorkOrderPrioritiesCatalog();

    expect(mockList).toHaveBeenCalledTimes(1);
    expect(useWorkOrderPrioritiesCatalog.getState().priorities).toEqual(rows);
  });
});
