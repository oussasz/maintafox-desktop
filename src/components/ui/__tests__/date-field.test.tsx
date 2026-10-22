import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";

import { DateField } from "@/components/ui/date-field";
import { DateTimeField } from "@/components/ui/date-time-field";
import { TimeInput } from "@/components/ui/time-input";
import { i18n, initI18n } from "@/i18n";

beforeAll(() => initI18n());
beforeEach(async () => {
  await i18n.changeLanguage("fr");
});

function Harness({ initial = "", min, max }: { initial?: string; min?: string; max?: string }) {
  const [value, setValue] = useState(initial);
  return (
    <>
      <DateField id="d" value={value} onChange={setValue} min={min} max={max} />
      <output data-testid="value">{value}</output>
    </>
  );
}

const box = () => screen.getByRole("textbox") as HTMLInputElement;
const value = () => screen.getByTestId("value").textContent;

describe("DateField", () => {
  it("shows an ISO value as dd/mm/yyyy in French and keeps the stored value ISO", () => {
    render(<Harness initial="2026-10-25" />);
    expect(box().value).toBe("25/10/2026");
    expect(value()).toBe("2026-10-25");
  });

  it("reads a typed date into an ISO value", () => {
    render(<Harness />);
    fireEvent.change(box(), { target: { value: "03/11/2026" } });
    expect(value()).toBe("2026-11-03");
  });

  it("holds no value while the text is unfinished or impossible", () => {
    render(<Harness initial="2026-10-25" />);
    fireEvent.change(box(), { target: { value: "03/11/2" } });
    expect(value()).toBe("");
    fireEvent.change(box(), { target: { value: "31/02/2026" } });
    expect(value()).toBe("");
    expect(box()).toHaveAttribute("aria-invalid", "true");
  });

  it("counts a pasted ISO date or a short year at once, so Enter in a form carries it", () => {
    render(<Harness />);
    fireEvent.change(box(), { target: { value: "2026-10-25" } });
    expect(value()).toBe("2026-10-25");
    fireEvent.change(box(), { target: { value: "5.1.26" } });
    expect(value()).toBe("2026-01-05");
  });

  it("flags a date outside the bounds and passes it on, so the form can say why it refuses it", () => {
    render(<Harness min="2026-10-01" max="2026-10-31" />);
    fireEvent.change(box(), { target: { value: "05/11/2026" } });
    expect(value()).toBe("2026-11-05");
    expect(box()).toHaveAttribute("aria-invalid", "true");
    fireEvent.change(box(), { target: { value: "05/10/2026" } });
    expect(value()).toBe("2026-10-05");
  });

  it("picks a day from the calendar", () => {
    render(<Harness initial="2026-10-25" />);
    fireEvent.click(screen.getByRole("button", { name: "Choisir une date" }));
    const day = screen.getAllByRole("button").find((b) => b.textContent === "12");
    expect(day).toBeDefined();
    fireEvent.pointerDown(day as HTMLElement);
    expect(value()).toBe("2026-10-12");
    expect(box().value).toBe("12/10/2026");
  });

  it("follows the app language", async () => {
    await i18n.changeLanguage("en");
    render(<Harness initial="2026-10-25" />);
    expect(box().value).toBe("10/25/2026");
  });
});

describe("TimeInput", () => {
  it("reads a typed 24-hour time and keeps the value HH:mm", () => {
    function T() {
      const [v, setV] = useState("");
      return (
        <>
          <TimeInput value={v} onChange={setV} />
          <output data-testid="value">{v}</output>
        </>
      );
    }
    render(<T />);
    fireEvent.change(box(), { target: { value: "8h05" } });
    expect(value()).toBe("08:05");
    fireEvent.change(box(), { target: { value: "2560" } });
    expect(value()).toBe("");
  });
});

describe("DateTimeField", () => {
  it("combines date and time into YYYY-MM-DDTHH:mm and waits for both parts", () => {
    function T() {
      const [v, setV] = useState("");
      return (
        <>
          <DateTimeField id="dt" value={v} onChange={setV} />
          <output data-testid="value">{v}</output>
        </>
      );
    }
    render(<T />);
    const [dateBox, timeBox] = screen.getAllByRole("textbox") as HTMLInputElement[];
    fireEvent.change(dateBox as HTMLInputElement, { target: { value: "25/10/2026" } });
    expect(value()).toBe("2026-10-25T08:00");
    fireEvent.change(timeBox as HTMLInputElement, { target: { value: "14:30" } });
    expect(value()).toBe("2026-10-25T14:30");
    fireEvent.change(timeBox as HTMLInputElement, { target: { value: "14:" } });
    expect(value()).toBe("");
    expect((dateBox as HTMLInputElement).value).toBe("25/10/2026");
  });
});
