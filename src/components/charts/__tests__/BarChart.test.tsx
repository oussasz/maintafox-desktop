// src/components/charts/__tests__/BarChart.test.tsx
// Integration tests: verifies BarChart renders SVG rect elements proportionally.

import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";

import { BarChart, type BarChartDatum } from "../BarChart";
import { CHART_COLORS } from "../chart-utils";

const SAMPLE_DATA: BarChartDatum[] = [
  { label: "Jan", value: 100 },
  { label: "Feb", value: 200 },
  { label: "Mar", value: 150 },
  { label: "Apr", value: 300 },
];

describe("BarChart", () => {
  it("renders an SVG element with role='img'", () => {
    const { container } = render(<BarChart data={SAMPLE_DATA} width={400} height={300} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg?.getAttribute("role")).toBe("img");
  });

  it("renders the correct number of rect elements", () => {
    const { container } = render(<BarChart data={SAMPLE_DATA} width={400} height={300} />);
    const rects = container.querySelectorAll("rect");
    expect(rects).toHaveLength(SAMPLE_DATA.length);
  });

  it("uses the default primary color when no color prop is given", () => {
    const { container } = render(<BarChart data={SAMPLE_DATA} width={400} height={300} />);
    const firstRect = container.querySelector("rect");
    expect(firstRect?.getAttribute("fill")).toBe(CHART_COLORS.primary);
  });

  it("uses a custom color when provided", () => {
    const { container } = render(
      <BarChart data={SAMPLE_DATA} width={400} height={300} color={CHART_COLORS.success} />,
    );
    const firstRect = container.querySelector("rect");
    expect(firstRect?.getAttribute("fill")).toBe(CHART_COLORS.success);
  });

  it("tallest bar corresponds to the largest value", () => {
    const { container } = render(<BarChart data={SAMPLE_DATA} width={400} height={300} />);
    const rects = Array.from(container.querySelectorAll("rect"));
    const heights = rects.map((r) => Number(r.getAttribute("height")));
    // The 4th item (value: 300) should have the tallest bar
    const maxIndex = heights.indexOf(Math.max(...heights));
    expect(maxIndex).toBe(3);
  });

  it("renders nothing visible when data is empty", () => {
    const { container } = render(<BarChart data={[]} width={400} height={300} />);
    const rects = container.querySelectorAll("rect");
    expect(rects).toHaveLength(0);
  });
});
