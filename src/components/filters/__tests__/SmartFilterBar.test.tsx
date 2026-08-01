import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) => {
      const map: Record<string, string> = {
        "smartFilter.reset": "Reset filters",
        "smartFilter.all": "All",
        "smartFilter.activeCount": `${opts?.["count"] ?? ""} active`,
        "smartFilter.resultCount": `${opts?.["count"] ?? ""} result(s)`,
        "smartFilter.clearSearch": "Clear search",
        "smartFilter.removeFilter": `Remove ${opts?.["label"] ?? ""} filter`,
        "smartFilter.searchChip": `Search: ${opts?.["query"] ?? ""}`,
        "smartFilter.addFilter": "Add…",
        "smartFilter.noMoreOptions": "No more options",
      };
      return map[key] ?? key;
    },
    i18n: { language: "en" },
  }),
}));

describe("SmartFilterBar", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("debounces search and emits settled value after 300ms", () => {
    const onSearchChange = vi.fn();
    const onSearchInputChange = vi.fn();

    function Harness() {
      const [search, setSearch] = useState("");
      return (
        <SmartFilterBar
          searchPlaceholder="Search items"
          searchValue={search}
          onSearchInputChange={(v) => {
            onSearchInputChange(v);
            setSearch(v);
          }}
          onSearchChange={onSearchChange}
          filters={[]}
          onReset={() => setSearch("")}
        />
      );
    }

    render(<Harness />);

    const input = screen.getByPlaceholderText("Search items");
    fireEvent.change(input, { target: { value: "pump" } });

    expect(onSearchInputChange).toHaveBeenCalledWith("pump");
    expect(onSearchChange).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(300);
    });

    expect(onSearchChange).toHaveBeenCalledWith("pump");
  });

  it("shows search chip and clears search immediately", () => {
    const onSearchChange = vi.fn();

    render(
      <SmartFilterBar
        searchPlaceholder="Search"
        searchValue="motor"
        onSearchChange={onSearchChange}
        filters={[]}
        onReset={vi.fn()}
      />,
    );

    expect(screen.getByText("Search: motor")).toBeInTheDocument();
    expect(screen.getByText("1 active")).toBeInTheDocument();

    const clearSearch = screen.getAllByLabelText("Clear search")[0];
    if (!clearSearch) {
      throw new Error("expected Clear search control");
    }
    fireEvent.click(clearSearch);
    expect(onSearchChange).toHaveBeenCalledWith("");
  });

  it("shows result count and active filter chip; reset invokes onReset", () => {
    const onStatusChange = vi.fn();
    const onReset = vi.fn();

    const filters: SmartFilterDef[] = [
      {
        id: "status",
        kind: "select",
        label: "Status",
        options: [
          { value: "open", label: "Open" },
          { value: "closed", label: "Closed" },
        ],
        value: "open",
        onChange: onStatusChange,
      },
    ];

    render(
      <SmartFilterBar
        searchPlaceholder="Search"
        searchValue=""
        onSearchChange={vi.fn()}
        filters={filters}
        resultCount={12}
        onReset={onReset}
      />,
    );

    expect(screen.getByText("12 result(s)")).toBeInTheDocument();
    expect(screen.getByText("Status:")).toBeInTheDocument();
    expect(screen.getAllByText("Open").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("1 active")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Reset filters" }));
    expect(onReset).toHaveBeenCalledTimes(1);
  });

  it("removes a select chip via the chip clear button", () => {
    const onStatusChange = vi.fn();

    const filters: SmartFilterDef[] = [
      {
        id: "status",
        kind: "select",
        label: "Status",
        options: [{ value: "open", label: "Open" }],
        value: "open",
        onChange: onStatusChange,
      },
    ];

    render(
      <SmartFilterBar
        searchPlaceholder="Search"
        searchValue=""
        onSearchChange={vi.fn()}
        filters={filters}
        onReset={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByLabelText("Remove Status filter"));
    expect(onStatusChange).toHaveBeenCalledWith(null);
  });
});
