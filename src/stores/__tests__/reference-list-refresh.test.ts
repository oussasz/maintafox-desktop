/**
 * Kept copies of plant lists follow Données de référence: a publish or an edit in this app reads
 * them again at once; a screen opening on an old copy reads it again; a recent copy is reused.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@/lib/ipc-invoke", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  invoke: invokeMock,
}));

const personnelSvc = vi.hoisted(() => ({ listPersonnelReferenceOptions: vi.fn() }));
vi.mock("@/services/personnel-service", () => personnelSvc);
const docSvc = vi.hoisted(() => ({ listDocumentCategories: vi.fn() }));
vi.mock("@/services/documentation-service", () => docSvc);

import { REFERENCE_LIST_MAX_AGE_MS } from "@/lib/reference-data-events";
import { publishReferenceSet, updateReferenceValue } from "@/services/reference-service";
import { useDocumentCategoriesStore } from "@/stores/document-categories-store";
import { usePersonnelListsStore } from "@/stores/personnel-lists-store";

const LISTS = {
  employment_statuses: [],
  employment_types: [],
  absence_types: [],
  position_categories: [],
};
const SET = {
  id: 4,
  domain_id: 2,
  version_no: 3,
  status: "published",
  effective_from: null,
  created_by_id: 1,
  created_at: "2026-10-18T08:00:00Z",
  published_at: "2026-10-18T09:00:00Z",
};

async function settle() {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

beforeEach(async () => {
  vi.clearAllMocks();
  personnelSvc.listPersonnelReferenceOptions.mockResolvedValue(LISTS);
  docSvc.listDocumentCategories.mockResolvedValue([]);
  usePersonnelListsStore.setState({ options: null, loadedAt: 0, loading: false, error: null });
  useDocumentCategoriesStore.setState({
    categories: null,
    loadedAt: 0,
    loading: false,
    error: null,
  });
  await usePersonnelListsStore.getState().load();
  await useDocumentCategoriesStore.getState().load();
  vi.clearAllMocks();
});

describe("plant list copies", () => {
  it("are read again when a list is published", async () => {
    invokeMock.mockResolvedValueOnce(SET);
    await publishReferenceSet(4);
    await settle();
    expect(personnelSvc.listPersonnelReferenceOptions).toHaveBeenCalledTimes(1);
    expect(docSvc.listDocumentCategories).toHaveBeenCalledTimes(1);
  });

  it("are not read again when the change was refused", async () => {
    invokeMock.mockRejectedValueOnce({ code: "VALIDATION_FAILED", message: "no" });
    await expect(updateReferenceValue(9, { label: "x" } as never)).rejects.toBeTruthy();
    await settle();
    expect(personnelSvc.listPersonnelReferenceOptions).not.toHaveBeenCalled();
    expect(docSvc.listDocumentCategories).not.toHaveBeenCalled();
  });

  it("are reused when recent, read again when a screen opens on an old copy", async () => {
    await usePersonnelListsStore.getState().load();
    expect(personnelSvc.listPersonnelReferenceOptions).not.toHaveBeenCalled();

    usePersonnelListsStore.setState({ loadedAt: Date.now() - REFERENCE_LIST_MAX_AGE_MS - 1 });
    useDocumentCategoriesStore.setState({ loadedAt: Date.now() - REFERENCE_LIST_MAX_AGE_MS - 1 });
    await usePersonnelListsStore.getState().load();
    await useDocumentCategoriesStore.getState().load();
    expect(personnelSvc.listPersonnelReferenceOptions).toHaveBeenCalledTimes(1);
    expect(docSvc.listDocumentCategories).toHaveBeenCalledTimes(1);
  });

  it("read again after a change that arrives while a read is under way", async () => {
    let finish: (v: typeof LISTS) => void = () => undefined;
    personnelSvc.listPersonnelReferenceOptions.mockImplementationOnce(
      () => new Promise((resolve) => (finish = resolve)),
    );
    const first = usePersonnelListsStore.getState().load(true);
    invokeMock.mockResolvedValueOnce(SET);
    await publishReferenceSet(4);
    finish(LISTS);
    await first;
    await settle();
    expect(personnelSvc.listPersonnelReferenceOptions).toHaveBeenCalledTimes(2);
  });
});
