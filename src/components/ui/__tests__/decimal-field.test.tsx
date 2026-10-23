import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeAll, describe, expect, it } from "vitest";

import { DecimalField } from "@/components/ui/decimal-field";
import { initI18n } from "@/i18n";
import { useLocaleStore } from "@/stores/locale-store";
import { formatDecimalInput, parseDecimalInput } from "@/utils/decimal";
import { formatBytes, formatCurrency, formatHours } from "@/utils/formatters";

const NBSP = String.fromCharCode(0xa0);
const NNBSP = String.fromCharCode(0x202f);

describe("parseDecimalInput", () => {
  it("accepts the separator of either language and spaces as thousands separators", () => {
    expect(parseDecimalInput("2,5")).toEqual({ kind: "number", value: 2.5 });
    expect(parseDecimalInput("2.5")).toEqual({ kind: "number", value: 2.5 });
    expect(parseDecimalInput("1 250,5")).toEqual({ kind: "number", value: 1250.5 });
    expect(parseDecimalInput(",5")).toEqual({ kind: "number", value: 0.5 });
    expect(parseDecimalInput("")).toEqual({ kind: "empty" });
  });

  it("refuses exponents, two separators and letters", () => {
    for (const bad of ["5e-7", "1,2,3", "1.2.3", "abc", "-1", "1,2a"]) {
      expect(parseDecimalInput(bad)).toEqual({ kind: "invalid" });
    }
    expect(parseDecimalInput("-1", { allowNegative: true })).toEqual({ kind: "number", value: -1 });
  });
});

describe("formatDecimalInput", () => {
  it("shows the app language separator, no grouping, no exponent", () => {
    expect(formatDecimalInput(1250.5, "fr")).toBe("1250,5");
    expect(formatDecimalInput(1250.5, "en")).toBe("1250.5");
    expect(formatDecimalInput(0.0000005, "fr", { decimals: 3 })).toBe("0");
    expect(formatDecimalInput(1250, "fr", { fixed: true })).toBe("1250,00");
    expect(formatDecimalInput(null, "fr")).toBe("");
  });
});

describe("shared formatters", () => {
  it("writes hours, bytes and the plant currency the French way", () => {
    expect(formatHours(3.5, "fr")).toBe(`3,5${NBSP}h`);
    expect(formatHours(3.5, "en")).toBe(`3.5${NBSP}h`);
    expect(formatBytes(1.5 * 1024 * 1024, "fr")).toBe(`1,5${NBSP}Mo`);
    expect(formatBytes(1.5 * 1024 * 1024, "en")).toBe(`1.5${NBSP}MB`);
    expect(formatBytes(512, "fr")).toBe(`512${NBSP}o`);
    expect(formatCurrency(1250, "fr", "DZD", "DA")).toBe(`1${NNBSP}250,00${NBSP}DA`);
    expect(formatCurrency(1250, "fr", "DZD")).toContain("DZD");
  });
});

function Harness({ initial }: { initial: number | null }) {
  const [v, setV] = useState<number | null>(initial);
  return (
    <>
      <DecimalField aria-label="qty" value={v} onValueChange={setV} decimals={3} />
      <output data-testid="out">{v === null ? "null" : String(v)}</output>
    </>
  );
}

describe("DecimalField", () => {
  beforeAll(() => {
    initI18n();
    useLocaleStore.setState({ activeLocale: "fr" });
  });

  it("shows a stored number with a comma and lets the user type either separator", () => {
    render(<Harness initial={2.5} />);
    const input = screen.getByLabelText("qty") as HTMLInputElement;
    expect(input.value).toBe("2,5");
    fireEvent.change(input, { target: { value: "0,125" } });
    expect(screen.getByTestId("out").textContent).toBe("0.125");
    fireEvent.change(input, { target: { value: "7.5" } });
    expect(screen.getByTestId("out").textContent).toBe("7.5");
  });

  it("keeps a half-typed value and flags text that is not a number", () => {
    render(<Harness initial={null} />);
    const input = screen.getByLabelText("qty") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "2," } });
    expect(input.value).toBe("2,");
    fireEvent.change(input, { target: { value: "2e5" } });
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(screen.getByTestId("out").textContent).toBe("2");
  });

  it("can be emptied, and rounds to its decimals when left", () => {
    render(<Harness initial={1} />);
    const input = screen.getByLabelText("qty") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "" } });
    expect(screen.getByTestId("out").textContent).toBe("null");
    fireEvent.change(input, { target: { value: "1,23456" } });
    fireEvent.blur(input);
    expect(screen.getByTestId("out").textContent).toBe("1.235");
    expect(input.value).toBe("1,235");
  });
});

describe("rounding and parent reconciliation", () => {
  it("rounds on the decimal digits, not the binary value", async () => {
    const { roundDecimal } = await import("@/utils/decimal");
    expect(roundDecimal(1.005, 2)).toBe(1.01);
    expect(roundDecimal(1.0005, 3)).toBe(1.001);
    expect(roundDecimal(2.5, 0)).toBe(3);
    expect(parseDecimalInput("-0", { allowNegative: true })).toEqual({ kind: "number", value: 0 });
  });

  it("shows the parent's value again when left with text that is not a number", () => {
    render(<Harness initial={5} />);
    const input = screen.getByLabelText("qty") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "abc" } });
    expect(input).toHaveAttribute("aria-invalid", "true");
    fireEvent.blur(input);
    expect(input.value).toBe("5");
    expect(input).not.toHaveAttribute("aria-invalid");
    expect(screen.getByTestId("out").textContent).toBe("5");
  });

  it("does not put a zero back while the user empties a field the parent keeps as 0", () => {
    function ZeroHarness() {
      const [v, setV] = useState<number>(5);
      return (
        <DecimalField aria-label="qty" value={v} onValueChange={(n) => setV(n ?? 0)} decimals={3} />
      );
    }
    render(<ZeroHarness />);
    const input = screen.getByLabelText("qty") as HTMLInputElement;
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "" } });
    expect(input.value).toBe("");
    fireEvent.change(input, { target: { value: "7" } });
    expect(input.value).toBe("7");
    fireEvent.change(input, { target: { value: "" } });
    fireEvent.blur(input);
    expect(input.value).toBe("");
  });
});
