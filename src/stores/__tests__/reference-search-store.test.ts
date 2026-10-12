/**
 * reference-search-store — behaviour tests.
 *
 * Mocks only the `@/services/reference-service` boundary.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

import { searchReferenceValues } from "@/services/reference-service";
import type { ReferenceSearchHit } from "@shared/ipc-types";

import { useReferenceSearchStore } from "../reference-search-store";

vi.mock("@/services/reference-service", () => ({
  searchReferenceValues: vi.fn(),
}));

const mockSearch = vi.mocked(searchReferenceValues);

// ── Fixtures ──────────────────────────────────────────────────────────────────

function makeHit(overrides: Partial<ReferenceSearchHit> = {}): ReferenceSearchHit {
  return {
    value_id: 1,
    code: "PUMP",
    label: "Pompe",
    matched_text: "pompe",
    match_source: "label",
    alias_type: null,
    rank: 1,
    ...overrides,
  };
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

const initialState = useReferenceSearchStore.getState();

function resetStore() {
  useReferenceSearchStore.setState(initialState, true);
}

beforeEach(() => {
  vi.clearAllMocks();
  mockSearch.mockReset();
  resetStore();
});

// ── Tests ─────────────────────────────────────────────────────────────────────

describe("reference-search-store — initial state", () => {
  it("starts with empty query, fr locale, no domain and no results", () => {
    const s = useReferenceSearchStore.getState();
    expect(s.query).toBe("");
    expect(s.locale).toBe("fr");
    expect(s.domain).toBe("");
    expect(s.results).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });
});

describe("reference-search-store — runSearch", () => {
  it("does not call the service when the query is blank and clears results/error", async () => {
    useReferenceSearchStore.setState({
      query: "   ",
      domain: "EQUIPMENT.FAMILY",
      results: [makeHit()],
      error: "old",
    });

    await useReferenceSearchStore.getState().runSearch();

    expect(mockSearch).not.toHaveBeenCalled();
    const s = useReferenceSearchStore.getState();
    expect(s.results).toEqual([]);
    expect(s.error).toBeNull();
    expect(s.loading).toBe(false);
  });

  it("does not call the service when no domain is selected", async () => {
    useReferenceSearchStore.setState({ query: "pompe", domain: "", results: [makeHit()] });

    await useReferenceSearchStore.getState().runSearch();

    expect(mockSearch).not.toHaveBeenCalled();
    expect(useReferenceSearchStore.getState().results).toEqual([]);
  });

  it("calls the service with (domain, query, locale) and stores the results", async () => {
    const hits = [makeHit(), makeHit({ value_id: 2, code: "VALVE", label: "Vanne", rank: 2 })];
    mockSearch.mockResolvedValueOnce(hits);
    useReferenceSearchStore.setState({ query: "pom", domain: "EQUIPMENT.FAMILY", locale: "en" });

    await useReferenceSearchStore.getState().runSearch();

    expect(mockSearch).toHaveBeenCalledTimes(1);
    expect(mockSearch).toHaveBeenCalledWith("EQUIPMENT.FAMILY", "pom", "en");
    expect(useReferenceSearchStore.getState().results).toEqual(hits);
  });

  it("passes the untrimmed query through to the service", async () => {
    mockSearch.mockResolvedValueOnce([]);
    useReferenceSearchStore.setState({ query: "  pom ", domain: "D" });

    await useReferenceSearchStore.getState().runSearch();

    expect(mockSearch).toHaveBeenCalledWith("D", "  pom ", "fr");
  });

  it("sets loading=true while in flight and false after resolution", async () => {
    const d = deferred<ReferenceSearchHit[]>();
    mockSearch.mockReturnValueOnce(d.promise);
    useReferenceSearchStore.setState({ query: "pom", domain: "D", error: "stale" });

    const pending = useReferenceSearchStore.getState().runSearch();
    expect(useReferenceSearchStore.getState().loading).toBe(true);
    expect(useReferenceSearchStore.getState().error).toBeNull();

    d.resolve([makeHit()]);
    await pending;

    expect(useReferenceSearchStore.getState().loading).toBe(false);
  });

  it("clears results and stores the error message when the service rejects (swallowed)", async () => {
    mockSearch.mockRejectedValueOnce({ code: "DB", message: "search failed" });
    useReferenceSearchStore.setState({ query: "pom", domain: "D", results: [makeHit()] });

    await expect(useReferenceSearchStore.getState().runSearch()).resolves.toBeUndefined();

    const s = useReferenceSearchStore.getState();
    expect(s.results).toEqual([]);
    expect(s.error).toBe("search failed");
    expect(s.loading).toBe(false);
  });

  it("uses Error.message for Error rejections", async () => {
    mockSearch.mockRejectedValueOnce(new Error("boom"));
    useReferenceSearchStore.setState({ query: "pom", domain: "D" });

    await useReferenceSearchStore.getState().runSearch();

    expect(useReferenceSearchStore.getState().error).toBe("boom");
  });

  it("keeps loading while the newer search runs, even if an older one finishes first", async () => {
    const first = deferred<ReferenceSearchHit[]>();
    const second = deferred<ReferenceSearchHit[]>();
    mockSearch.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    useReferenceSearchStore.setState({ domain: "D" });

    const p1 = useReferenceSearchStore.getState().setQuery("p");
    const p2 = useReferenceSearchStore.getState().setQuery("po");
    first.resolve([makeHit({ value_id: 1, code: "P" })]);
    await p1;
    expect(useReferenceSearchStore.getState().loading).toBe(true);
    expect(useReferenceSearchStore.getState().results).toEqual([]);
    second.resolve([makeHit({ value_id: 2, code: "POMPE" })]);
    await p2;
    expect(useReferenceSearchStore.getState().loading).toBe(false);
  });

  it("drops a late result after the query was cleared", async () => {
    const first = deferred<ReferenceSearchHit[]>();
    mockSearch.mockReturnValueOnce(first.promise);
    useReferenceSearchStore.setState({ domain: "D" });
    const p1 = useReferenceSearchStore.getState().setQuery("p");
    await useReferenceSearchStore.getState().setQuery("");
    first.resolve([makeHit()]);
    await p1;
    expect(useReferenceSearchStore.getState().results).toEqual([]);
  });

  it("a stale (older) search response never overwrites results of a newer query", async () => {
    const first = deferred<ReferenceSearchHit[]>();
    const second = deferred<ReferenceSearchHit[]>();
    mockSearch.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    useReferenceSearchStore.setState({ domain: "D" });

    const p1 = useReferenceSearchStore.getState().setQuery("p");
    const p2 = useReferenceSearchStore.getState().setQuery("po");

    const newer = [makeHit({ value_id: 2, code: "POMPE" })];
    second.resolve(newer);
    await p2;
    first.resolve([makeHit({ value_id: 1, code: "P" })]);
    await p1;

    expect(useReferenceSearchStore.getState().query).toBe("po");
    expect(useReferenceSearchStore.getState().results).toEqual(newer);
  });
});

describe("reference-search-store — setQuery", () => {
  it("updates the query and runs the search", async () => {
    mockSearch.mockResolvedValueOnce([makeHit()]);
    useReferenceSearchStore.setState({ domain: "D" });

    await useReferenceSearchStore.getState().setQuery("pompe");

    expect(useReferenceSearchStore.getState().query).toBe("pompe");
    expect(mockSearch).toHaveBeenCalledWith("D", "pompe", "fr");
    expect(useReferenceSearchStore.getState().results).toHaveLength(1);
  });

  it("clearing the query clears results without calling the service", async () => {
    useReferenceSearchStore.setState({ domain: "D", query: "x", results: [makeHit()] });

    await useReferenceSearchStore.getState().setQuery("");

    expect(mockSearch).not.toHaveBeenCalled();
    expect(useReferenceSearchStore.getState().query).toBe("");
    expect(useReferenceSearchStore.getState().results).toEqual([]);
  });
});

describe("reference-search-store — setLocale", () => {
  it("updates locale and re-runs the search when the query is non-empty", async () => {
    mockSearch.mockResolvedValueOnce([]);
    useReferenceSearchStore.setState({ domain: "D", query: "pump" });

    await useReferenceSearchStore.getState().setLocale("en");

    expect(useReferenceSearchStore.getState().locale).toBe("en");
    expect(mockSearch).toHaveBeenCalledWith("D", "pump", "en");
  });

  it("updates locale without searching when the query is blank", async () => {
    useReferenceSearchStore.setState({ domain: "D", query: "  ", results: [makeHit()] });

    await useReferenceSearchStore.getState().setLocale("en");

    expect(useReferenceSearchStore.getState().locale).toBe("en");
    expect(mockSearch).not.toHaveBeenCalled();
    // results are left untouched when no search runs
    expect(useReferenceSearchStore.getState().results).toHaveLength(1);
  });
});

describe("reference-search-store — setDomain", () => {
  it("updates domain and re-runs the search when the query is non-empty", async () => {
    const hits = [makeHit({ code: "X" })];
    mockSearch.mockResolvedValueOnce(hits);
    useReferenceSearchStore.setState({ query: "x" });

    await useReferenceSearchStore.getState().setDomain("FAILURE.MODE");

    expect(useReferenceSearchStore.getState().domain).toBe("FAILURE.MODE");
    expect(mockSearch).toHaveBeenCalledWith("FAILURE.MODE", "x", "fr");
    expect(useReferenceSearchStore.getState().results).toEqual(hits);
  });

  it("updates domain without searching when the query is empty", async () => {
    await useReferenceSearchStore.getState().setDomain("FAILURE.MODE");

    expect(useReferenceSearchStore.getState().domain).toBe("FAILURE.MODE");
    expect(mockSearch).not.toHaveBeenCalled();
  });

  it("clearing the domain with a non-empty query empties results without calling the service", async () => {
    useReferenceSearchStore.setState({ query: "x", domain: "D", results: [makeHit()] });

    await useReferenceSearchStore.getState().setDomain("");

    expect(mockSearch).not.toHaveBeenCalled();
    expect(useReferenceSearchStore.getState().results).toEqual([]);
  });
});

describe("reference-search-store — reset", () => {
  it("restores every field to its default", () => {
    useReferenceSearchStore.setState({
      query: "x",
      locale: "en",
      domain: "D",
      results: [makeHit()],
      loading: true,
      error: "err",
    });

    useReferenceSearchStore.getState().reset();

    const s = useReferenceSearchStore.getState();
    expect(s.query).toBe("");
    expect(s.locale).toBe("fr");
    expect(s.domain).toBe("");
    expect(s.results).toEqual([]);
    expect(s.loading).toBe(false);
    expect(s.error).toBeNull();
  });
});
