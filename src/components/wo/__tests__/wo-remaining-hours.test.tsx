/**
 * The optional estimated remaining time of a resume (and a reopening to in progress): empty means
 * the full expected duration; otherwise a number of hours greater than 0 (decimal comma or point).
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "fr" },
  }),
}));

import { parseRemainingHours } from "../WoRemainingHoursField";
import { WoResumeDialog } from "../WoResumeDialog";

describe("parseRemainingHours", () => {
  it("reads empty as no value and a positive number with a comma or a point", () => {
    expect(parseRemainingHours("")).toBeNull();
    expect(parseRemainingHours("   ")).toBeNull();
    expect(parseRemainingHours("2,5")).toBe(2.5);
    expect(parseRemainingHours(" 2.5 ")).toBe(2.5);
    expect(parseRemainingHours("4")).toBe(4);
  });

  it("refuses zero, negative and non-numeric values", () => {
    for (const bad of ["0", "0,0", "-1", "abc", "1e3", "2,5,1", "Infinity"]) {
      expect(parseRemainingHours(bad)).toBe("invalid");
    }
  });
});

describe("WoResumeDialog", () => {
  function setup() {
    const onConfirm = vi.fn();
    render(<WoResumeDialog open busy={false} onOpenChange={vi.fn()} onConfirm={onConfirm} />);
    const input = screen.getByLabelText("remainingTime.label");
    const resume = screen.getByRole("button", { name: "footer.resume" });
    return { onConfirm, input, resume };
  }

  it("resumes with the full expected duration when the field is left empty", () => {
    const { onConfirm, resume } = setup();
    fireEvent.click(resume);
    expect(onConfirm).toHaveBeenCalledWith(null);
  });

  it("sends the remaining time entered", () => {
    const { onConfirm, input, resume } = setup();
    fireEvent.change(input, { target: { value: "1,5" } });
    fireEvent.click(resume);
    expect(onConfirm).toHaveBeenCalledWith(1.5);
  });

  it("refuses a remaining time of 0 or less: explained, and nothing is sent", () => {
    const { onConfirm, input, resume } = setup();
    for (const bad of ["0", "-2"]) {
      fireEvent.change(input, { target: { value: bad } });
      expect(screen.getByText("remainingTime.invalid")).toBeInTheDocument();
      expect(input).toHaveAttribute("aria-invalid", "true");
      expect(resume).toBeDisabled();
      fireEvent.click(resume);
    }
    expect(onConfirm).not.toHaveBeenCalled();
  });
});
