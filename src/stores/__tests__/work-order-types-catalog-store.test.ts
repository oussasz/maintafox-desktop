/**
 * Behaviour tests for work-order-types-catalog-store.
 * Only the wo-service boundary is mocked.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { listWorkOrderTypes } from "@/services/wo-service";
import type { WorkOrderTypeOption } from "@shared/ipc-types";

import {
  refreshWorkOrderTypesCatalog,
  useWorkOrderTypesCatalog,
} from "../work-order-types-catalog-store";

vi.mock("@/services/wo-service", () => ({
  listWorkOrderTypes: vi.fn(),
}));

const mockList = vi.mocked(listWorkOrderTypes);

function makeType(overrides: Partial<WorkOrderTypeOption> = {}): WorkOrderTypeOption {
  return {
    id: 1,
    code: "CORR",
    label: "Corrective",
    is_system: true,
    is_active: true,
    ...overrides,
  };
}

const initial = useWorkOrderTypesCatalog.getState();

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockReset();
  useWorkOrderTypesCatalog.setState(initial, true);
});

describe("work-order-types-catalog-store", () => {
  it("starts empty and idle", () => {
    const s = useWorkOrderTypesCatalog.getState();
    expect(s.types).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });

  it("load sets loading while in flight, then stores the types", async () => {
    let resolve: (v: WorkOrderTypeOption[]) => void = () => undefined;
    mockList.mockReturnValueOnce(
      new Promise<WorkOrderTypeOption[]>((r) => {
        resolve = r;
      }),
    );
    useWorkOrderTypesCatalog.setState({ error: "stale" });

    const p = useWorkOrderTypesCatalog.getState().load();
    expect(useWorkOrderTypesCatalog.getState().loading).toBe(true);
    expect(useWorkOrderTypesCatalog.getState().error).toBeNull();

    const types = [makeType(), makeType({ id: 2, code: "PREV", label: "Preventive" })];
    resolve(types);
    await p;

    const s = useWorkOrderTypesCatalog.getState();
    expect(s.loading).toBe(false);
    expect(s.types).toEqual(types);
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(mockList).toHaveBeenCalledWith();
  });

  it("load swallows errors, sets the message and keeps previous types", async () => {
    const previous = [makeType()];
    useWorkOrderTypesCatalog.setState({ types: previous });
    mockList.mockRejectedValueOnce({ code: "DB", message: "db down" });

    await expect(useWorkOrderTypesCatalog.getState().load()).resolves.toBeUndefined();

    const s = useWorkOrderTypesCatalog.getState();
    expect(s.loading).toBe(false);
    expect(s.error).toBe("db down");
    expect(s.types).toBe(previous);
  });

  it("refreshWorkOrderTypesCatalog delegates to the store load action", async () => {
    const types = [makeType({ id: 9, code: "INSP" })];
    mockList.mockResolvedValueOnce(types);

    await refreshWorkOrderTypesCatalog();

    expect(mockList).toHaveBeenCalledTimes(1);
    expect(useWorkOrderTypesCatalog.getState().types).toEqual(types);
  });
});
