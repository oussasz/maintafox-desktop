/**
 * personnel-store — behaviour tests.
 *
 * Mocks only the `@/services/personnel-service` boundary.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  createPersonnel,
  deactivatePersonnel,
  getPersonnel,
  listPersonnel,
  updatePersonnel,
} from "@/services/personnel-service";
import type {
  Personnel,
  PersonnelCreateInput,
  PersonnelDetailPayload,
  PersonnelListPage,
  PersonnelUpdateInput,
} from "@shared/ipc-types";

import { usePersonnelStore } from "../personnel-store";

vi.mock("@/services/personnel-service", () => ({
  createPersonnel: vi.fn(),
  deactivatePersonnel: vi.fn(),
  getPersonnel: vi.fn(),
  listPersonnel: vi.fn(),
  updatePersonnel: vi.fn(),
}));

const mockCreate = vi.mocked(createPersonnel);
const mockDeactivate = vi.mocked(deactivatePersonnel);
const mockGet = vi.mocked(getPersonnel);
const mockList = vi.mocked(listPersonnel);
const mockUpdate = vi.mocked(updatePersonnel);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makePersonnel(overrides: Partial<Personnel> = {}): Personnel {
  return {
    id: 1,
    employee_code: "EMP-0001",
    full_name: "Amine Benali",
    employment_type: "employee",
    employment_origin: "internal",
    employment_status: "active",
    blocked_override: false,
    position_id: 3,
    primary_entity_id: 5,
    primary_team_id: 7,
    supervisor_id: null,
    home_schedule_reference_value_id: 11,
    availability_status: "available",
    hire_date: "2024-01-15",
    termination_date: null,
    email: "amine@example.test",
    phone: null,
    photo_path: null,
    hr_external_id: null,
    external_company_id: null,
    contract_number: null,
    contract_start_date: null,
    contract_end_date: null,
    notes: null,
    row_version: 1,
    created_at: "2026-01-01T08:00:00Z",
    updated_at: "2026-01-01T08:00:00Z",
    position_name: "Technicien",
    position_code: "TECH",
    position_category: "technician",
    entity_name: "Atelier",
    team_name: "Équipe A",
    supervisor_name: null,
    schedule_name: "Jour",
    company_name: null,
    ...overrides,
  };
}

function makeDetail(personnel: Personnel = makePersonnel()): PersonnelDetailPayload {
  return { personnel, rate_cards: [], authorizations: [] };
}

function makePage(items: Personnel[], total = items.length): PersonnelListPage {
  return { items, total };
}

function makeCreateInput(overrides: Partial<PersonnelCreateInput> = {}): PersonnelCreateInput {
  return {
    full_name: "Nadia Kaci",
    employee_code: "EMP-0002",
    employment_type: "employee",
    employment_origin: "internal",
    position_id: 3,
    primary_entity_id: 5,
    primary_team_id: 7,
    home_schedule_reference_value_id: 11,
    ...overrides,
  };
}

function makeUpdateInput(overrides: Partial<PersonnelUpdateInput> = {}): PersonnelUpdateInput {
  return { id: 1, expected_row_version: 1, full_name: "Amine B.", ...overrides };
}

function deferred<T>() {
  let resolve: (value: T) => void = () => undefined;
  let reject: (reason: unknown) => void = () => undefined;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const initialState = usePersonnelStore.getState();

function resetStore() {
  usePersonnelStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockCreate.mockReset();
  mockDeactivate.mockReset();
  mockGet.mockReset();
  mockList.mockReset();
  mockUpdate.mockReset();
  resetStore();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("personnel-store — initial state", () => {
  it("starts empty with the default page filter", () => {
    const s = usePersonnelStore.getState();
    expect(s.items).toEqual([]);
    expect(s.total).toBe(0);
    expect(s.activePersonnel).toBeNull();
    expect(s.filter).toEqual({ limit: 50, offset: 0 });
    expect(s.loading).toBe(false);
    expect(s.saving).toBe(false);
    expect(s.error).toBeNull();
    expect(s.showCreateForm).toBe(false);
    expect(s.editingPersonnel).toBeNull();
  });
});

describe("personnel-store — setFilter", () => {
  it("merges partial updates into the existing filter", () => {
    usePersonnelStore.getState().setFilter({ search: "ami" });
    usePersonnelStore.getState().setFilter({ employment_status: ["active"] });

    expect(usePersonnelStore.getState().filter).toEqual({
      limit: 50,
      offset: 0,
      search: "ami",
      employment_status: ["active"],
    });
  });

  it("resets offset to 0 on every filter change", () => {
    usePersonnelStore.setState({ filter: { limit: 50, offset: 100 } });

    usePersonnelStore.getState().setFilter({ team_id: 7 });

    expect(usePersonnelStore.getState().filter.offset).toBe(0);
  });

  it("forces offset to 0 even when an explicit offset is passed (no paging via setFilter)", () => {
    usePersonnelStore.getState().setFilter({ offset: 50 });

    expect(usePersonnelStore.getState().filter.offset).toBe(0);
  });

  it("does not trigger a reload by itself", () => {
    usePersonnelStore.getState().setFilter({ search: "x" });

    expect(mockList).not.toHaveBeenCalled();
  });
});

describe("personnel-store — loadPersonnel", () => {
  it("calls listPersonnel with the current filter and stores items/total", async () => {
    const rows = [makePersonnel(), makePersonnel({ id: 2, employee_code: "EMP-0002" })];
    mockList.mockResolvedValueOnce(makePage(rows, 42));
    usePersonnelStore.getState().setFilter({ search: "a" });

    await usePersonnelStore.getState().loadPersonnel();

    expect(mockList).toHaveBeenCalledWith({ limit: 50, offset: 0, search: "a" });
    expect(usePersonnelStore.getState().items).toEqual(rows);
    expect(usePersonnelStore.getState().total).toBe(42);
  });

  it("toggles loading around the request and clears a previous error", async () => {
    const d = deferred<PersonnelListPage>();
    mockList.mockReturnValueOnce(d.promise);
    usePersonnelStore.setState({ error: "previous" });

    const pending = usePersonnelStore.getState().loadPersonnel();
    expect(usePersonnelStore.getState().loading).toBe(true);
    expect(usePersonnelStore.getState().error).toBeNull();

    d.resolve(makePage([]));
    await pending;
    expect(usePersonnelStore.getState().loading).toBe(false);
  });

  it("sets error and keeps existing items when the service rejects (swallowed)", async () => {
    const existing = [makePersonnel()];
    usePersonnelStore.setState({ items: existing, total: 1 });
    mockList.mockRejectedValueOnce({ code: "DB", message: "list failed" });

    await expect(usePersonnelStore.getState().loadPersonnel()).resolves.toBeUndefined();

    const s = usePersonnelStore.getState();
    expect(s.error).toBe("list failed");
    expect(s.items).toEqual(existing);
    expect(s.total).toBe(1);
    expect(s.loading).toBe(false);
  });
});

describe("personnel-store — openPersonnel / closePersonnel", () => {
  it("loads the detail payload into activePersonnel", async () => {
    const detail = makeDetail(makePersonnel({ id: 9 }));
    mockGet.mockResolvedValueOnce(detail);

    await usePersonnelStore.getState().openPersonnel(9);

    expect(mockGet).toHaveBeenCalledWith(9);
    expect(usePersonnelStore.getState().activePersonnel).toEqual(detail);
  });

  it("sets error, keeps the previous active record and does not rethrow on failure", async () => {
    const previous = makeDetail();
    usePersonnelStore.setState({ activePersonnel: previous });
    mockGet.mockRejectedValueOnce(new Error("not found"));

    await expect(usePersonnelStore.getState().openPersonnel(99)).resolves.toBeUndefined();

    expect(usePersonnelStore.getState().error).toBe("not found");
    expect(usePersonnelStore.getState().activePersonnel).toEqual(previous);
  });

  it("does not touch the loading or saving flags", async () => {
    mockGet.mockResolvedValueOnce(makeDetail());

    const pending = usePersonnelStore.getState().openPersonnel(1);
    expect(usePersonnelStore.getState().loading).toBe(false);
    expect(usePersonnelStore.getState().saving).toBe(false);
    await pending;
  });

  it("closePersonnel clears the active record", () => {
    usePersonnelStore.setState({ activePersonnel: makeDetail() });

    usePersonnelStore.getState().closePersonnel();

    expect(usePersonnelStore.getState().activePersonnel).toBeNull();
  });
});

describe("personnel-store — submitNewPersonnel", () => {
  it("creates, reloads the list, closes the form and returns the created record", async () => {
    const created = makePersonnel({ id: 2, employee_code: "EMP-0002" });
    const input = makeCreateInput();
    mockCreate.mockResolvedValueOnce(created);
    mockList.mockResolvedValueOnce(makePage([created]));
    usePersonnelStore.setState({ showCreateForm: true, editingPersonnel: makePersonnel() });

    const result = await usePersonnelStore.getState().submitNewPersonnel(input);

    expect(result).toEqual(created);
    expect(mockCreate).toHaveBeenCalledWith(input);
    expect(mockList).toHaveBeenCalledTimes(1);
    const s = usePersonnelStore.getState();
    expect(s.items).toEqual([created]);
    expect(s.showCreateForm).toBe(false);
    expect(s.editingPersonnel).toBeNull();
    expect(s.saving).toBe(false);
  });

  it("sets saving=true while the create request is in flight", async () => {
    const d = deferred<Personnel>();
    mockCreate.mockReturnValueOnce(d.promise);
    mockList.mockResolvedValueOnce(makePage([]));

    const pending = usePersonnelStore.getState().submitNewPersonnel(makeCreateInput());
    expect(usePersonnelStore.getState().saving).toBe(true);

    d.resolve(makePersonnel());
    await pending;
    expect(usePersonnelStore.getState().saving).toBe(false);
  });

  it("sets error, rethrows and keeps the form open when create fails", async () => {
    const failure = { code: "VALIDATION", message: "duplicate employee code" };
    mockCreate.mockRejectedValueOnce(failure);
    usePersonnelStore.setState({ showCreateForm: true });

    await expect(
      usePersonnelStore.getState().submitNewPersonnel(makeCreateInput()),
    ).rejects.toEqual(failure);

    const s = usePersonnelStore.getState();
    expect(s.error).toBe("duplicate employee code");
    expect(s.showCreateForm).toBe(true);
    expect(s.saving).toBe(false);
    expect(mockList).not.toHaveBeenCalled();
  });

  it("still resolves and closes the form when the post-create reload fails (reload error is surfaced)", async () => {
    const created = makePersonnel({ id: 3 });
    mockCreate.mockResolvedValueOnce(created);
    mockList.mockRejectedValueOnce(new Error("reload failed"));
    usePersonnelStore.setState({ showCreateForm: true });

    await expect(
      usePersonnelStore.getState().submitNewPersonnel(makeCreateInput()),
    ).resolves.toEqual(created);

    const s = usePersonnelStore.getState();
    expect(s.error).toBe("reload failed");
    expect(s.showCreateForm).toBe(false);
  });
});

describe("personnel-store — updateExisting", () => {
  it("updates, refreshes the open detail when it is the same record, and reloads the list", async () => {
    usePersonnelStore.setState({ activePersonnel: makeDetail(makePersonnel({ id: 1 })) });
    const updated = makePersonnel({ id: 1, full_name: "Amine B.", row_version: 2 });
    mockUpdate.mockResolvedValueOnce(updated);
    mockGet.mockResolvedValueOnce(makeDetail(updated));
    mockList.mockResolvedValueOnce(makePage([updated]));
    const input = makeUpdateInput();

    await usePersonnelStore.getState().updateExisting(input);

    expect(mockUpdate).toHaveBeenCalledWith(input);
    expect(mockGet).toHaveBeenCalledWith(1);
    expect(mockList).toHaveBeenCalledTimes(1);
    const s = usePersonnelStore.getState();
    expect(s.activePersonnel?.personnel.full_name).toBe("Amine B.");
    expect(s.items).toEqual([updated]);
    expect(s.saving).toBe(false);
  });

  it("does not refetch detail when a different record is open", async () => {
    const other = makeDetail(makePersonnel({ id: 5 }));
    usePersonnelStore.setState({ activePersonnel: other });
    mockUpdate.mockResolvedValueOnce(makePersonnel({ id: 1 }));
    mockList.mockResolvedValueOnce(makePage([]));

    await usePersonnelStore.getState().updateExisting(makeUpdateInput({ id: 1 }));

    expect(mockGet).not.toHaveBeenCalled();
    expect(usePersonnelStore.getState().activePersonnel).toEqual(other);
  });

  it("does not refetch detail when nothing is open", async () => {
    mockUpdate.mockResolvedValueOnce(makePersonnel());
    mockList.mockResolvedValueOnce(makePage([]));

    await usePersonnelStore.getState().updateExisting(makeUpdateInput());

    expect(mockGet).not.toHaveBeenCalled();
    expect(mockList).toHaveBeenCalledTimes(1);
  });

  it("sets saving while in flight", async () => {
    const d = deferred<Personnel>();
    mockUpdate.mockReturnValueOnce(d.promise);
    mockList.mockResolvedValueOnce(makePage([]));

    const pending = usePersonnelStore.getState().updateExisting(makeUpdateInput());
    expect(usePersonnelStore.getState().saving).toBe(true);

    d.resolve(makePersonnel());
    await pending;
    expect(usePersonnelStore.getState().saving).toBe(false);
  });

  it("sets error and rethrows on failure without reloading", async () => {
    const failure = { code: "CONFLICT", message: "row version mismatch" };
    mockUpdate.mockRejectedValueOnce(failure);

    await expect(usePersonnelStore.getState().updateExisting(makeUpdateInput())).rejects.toEqual(
      failure,
    );

    expect(usePersonnelStore.getState().error).toBe("row version mismatch");
    expect(usePersonnelStore.getState().saving).toBe(false);
    expect(mockList).not.toHaveBeenCalled();
    expect(mockGet).not.toHaveBeenCalled();
  });

  it("rethrows when the detail refresh fails after a successful update", async () => {
    usePersonnelStore.setState({ activePersonnel: makeDetail(makePersonnel({ id: 1 })) });
    mockUpdate.mockResolvedValueOnce(makePersonnel());
    const failure = new Error("detail failed");
    mockGet.mockRejectedValueOnce(failure);

    await expect(usePersonnelStore.getState().updateExisting(makeUpdateInput())).rejects.toBe(
      failure,
    );

    expect(usePersonnelStore.getState().error).toBe("detail failed");
    expect(mockList).not.toHaveBeenCalled();
  });
});

describe("personnel-store — deactivate", () => {
  it("passes id and row version, reloads the list and refreshes the open detail", async () => {
    usePersonnelStore.setState({ activePersonnel: makeDetail(makePersonnel({ id: 4 })) });
    const deactivated = makePersonnel({ id: 4, employment_status: "inactive", row_version: 3 });
    mockDeactivate.mockResolvedValueOnce(deactivated);
    mockList.mockResolvedValueOnce(makePage([deactivated]));
    mockGet.mockResolvedValueOnce(makeDetail(deactivated));

    await usePersonnelStore.getState().deactivate(4, 2);

    expect(mockDeactivate).toHaveBeenCalledWith(4, 2);
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(mockGet).toHaveBeenCalledWith(4);
    expect(usePersonnelStore.getState().activePersonnel?.personnel.employment_status).toBe(
      "inactive",
    );
    expect(usePersonnelStore.getState().saving).toBe(false);
  });

  it("does not refresh detail when a different record is open", async () => {
    const other = makeDetail(makePersonnel({ id: 8 }));
    usePersonnelStore.setState({ activePersonnel: other });
    mockDeactivate.mockResolvedValueOnce(makePersonnel({ id: 4 }));
    mockList.mockResolvedValueOnce(makePage([]));

    await usePersonnelStore.getState().deactivate(4, 1);

    expect(mockGet).not.toHaveBeenCalled();
    expect(usePersonnelStore.getState().activePersonnel).toEqual(other);
  });

  it("sets saving while in flight", async () => {
    const d = deferred<Personnel>();
    mockDeactivate.mockReturnValueOnce(d.promise);
    mockList.mockResolvedValueOnce(makePage([]));

    const pending = usePersonnelStore.getState().deactivate(1, 1);
    expect(usePersonnelStore.getState().saving).toBe(true);

    d.resolve(makePersonnel());
    await pending;
    expect(usePersonnelStore.getState().saving).toBe(false);
  });

  it("sets error and rethrows on failure without reloading", async () => {
    const failure = { code: "PERMISSION", message: "forbidden" };
    mockDeactivate.mockRejectedValueOnce(failure);

    await expect(usePersonnelStore.getState().deactivate(1, 1)).rejects.toEqual(failure);

    expect(usePersonnelStore.getState().error).toBe("forbidden");
    expect(usePersonnelStore.getState().saving).toBe(false);
    expect(mockList).not.toHaveBeenCalled();
  });
});

describe("personnel-store — create form state", () => {
  it("openCreateForm() without an argument opens an empty create form", () => {
    usePersonnelStore.getState().openCreateForm();

    expect(usePersonnelStore.getState().showCreateForm).toBe(true);
    expect(usePersonnelStore.getState().editingPersonnel).toBeNull();
  });

  it("openCreateForm(personnel) opens the form in edit mode", () => {
    const p = makePersonnel({ id: 6 });

    usePersonnelStore.getState().openCreateForm(p);

    expect(usePersonnelStore.getState().showCreateForm).toBe(true);
    expect(usePersonnelStore.getState().editingPersonnel).toEqual(p);
  });

  it("closeCreateForm hides the form and clears the edited record", () => {
    usePersonnelStore.setState({ showCreateForm: true, editingPersonnel: makePersonnel() });

    usePersonnelStore.getState().closeCreateForm();

    expect(usePersonnelStore.getState().showCreateForm).toBe(false);
    expect(usePersonnelStore.getState().editingPersonnel).toBeNull();
  });
});

describe("personnel-store — loadMore and raw load errors", () => {
  it("keeps the raw list failure for the error state", async () => {
    const failure = { code: "DATABASE_ERROR", message: "down" };
    mockList.mockRejectedValueOnce(failure);
    await usePersonnelStore.getState().loadPersonnel();
    expect(usePersonnelStore.getState().loadError).toBe(failure);
  });

  it("appends the next page from the current length and skips duplicates", async () => {
    const first = [makePersonnel(), makePersonnel({ id: 2, employee_code: "EMP-0002" })];
    usePersonnelStore.setState({ items: first, total: 3 });
    mockList.mockResolvedValueOnce(
      makePage([makePersonnel({ id: 2 }), makePersonnel({ id: 3, employee_code: "EMP-0003" })], 3),
    );

    await usePersonnelStore.getState().loadMore();

    expect(mockList).toHaveBeenCalledWith(expect.objectContaining({ offset: 2 }));
    expect(usePersonnelStore.getState().items.map((p) => p.id)).toEqual([1, 2, 3]);
    expect(usePersonnelStore.getState().loadingMore).toBe(false);
  });

  it("does nothing when everything is loaded", async () => {
    usePersonnelStore.setState({ items: [makePersonnel()], total: 1 });
    await usePersonnelStore.getState().loadMore();
    expect(mockList).not.toHaveBeenCalled();
  });

  it("keeps the loaded rows and the failure when the next page fails", async () => {
    const first = [makePersonnel()];
    const failure = { code: "DATABASE_ERROR", message: "down" };
    usePersonnelStore.setState({ items: first, total: 5 });
    mockList.mockRejectedValueOnce(failure);
    await usePersonnelStore.getState().loadMore();
    expect(usePersonnelStore.getState().items).toBe(first);
    expect(usePersonnelStore.getState().loadError).toBe(failure);
  });

  it("keeps the raw detail failure", async () => {
    const failure = { code: "NOT_FOUND", message: "gone" };
    mockGet.mockRejectedValueOnce(failure);
    await usePersonnelStore.getState().openPersonnel(9);
    expect(usePersonnelStore.getState().detailError).toBe(failure);
  });
});
