import { render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { SmartFilterBar } from "@/components/filters/SmartFilterBar";

const debounce = vi.hoisted(() => ({ value: "" }));

// Controlled debounce: lets the test settle the debounced value in the exact commit
// where the parent clears the search, which timers cannot do deterministically.
vi.mock("@/hooks/useDebouncedValue", () => ({
  useDebouncedValue: () => debounce.value,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en" },
  }),
}));

describe("SmartFilterBar stale emit", () => {
  beforeEach(() => {
    debounce.value = "";
  });

  it("does not re-emit the old search when the parent clears it as the debounce settles", () => {
    const onSearchChange = vi.fn();
    const ui = (searchValue: string) => (
      <SmartFilterBar
        searchPlaceholder="Search"
        searchValue={searchValue}
        onSearchChange={onSearchChange}
        filters={[]}
        onReset={vi.fn()}
      />
    );

    const { rerender } = render(ui(""));

    // The page sets a search from code.
    rerender(ui("seal"));

    // The debounce for "seal" settles in the same commit where the page clears it.
    debounce.value = "seal";
    rerender(ui(""));

    // The debounce then catches up with the cleared text.
    debounce.value = "";
    rerender(ui(""));

    expect(onSearchChange).not.toHaveBeenCalledWith("seal");
    expect(onSearchChange).not.toHaveBeenCalled();
  });
});
