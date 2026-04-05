// src/components/charts/__tests__/LineChart.test.tsx
// Integration tests: verifies LineChart renders an SVG path and optional dots.

import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";

import { LineChart, type LineChartDatum } from "../LineChart";
import { CHART_COLORS } from "../chart-utils";

const SAMPLE_DATA: LineChartDatum[] = [
  { x: 0, y: 10 },
  { x: 1, y: 40 },
  { x: 2, y: 25 },
  { x: 3, y: 60 },
  { x: 4, y: 35 },
];

describe("LineChart", () => {
  it("renders an SVG element with role='img'", () => {
    const { container } = render(<LineChart data={SAMPLE_DATA} width={400} height={300} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg?.getAttribute("role")).toBe("img");
  });

  it("renders a path element for the line", () => {
    const { container } = render(<LineChart data={SAMPLE_DATA} width={400} height={300} />);
    const path = container.querySelector("path");
    expect(path).not.toBeNull();
    expect(path?.getAttribute("d")).toBeTruthy();
    expect(path?.getAttribute("fill")).toBe("none");
  });

  it("uses the default primary color for the stroke", () => {
    const { container } = render(<LineChart data={SAMPLE_DATA} width={400} height={300} />);
    const path = container.querySelector("path");
    expect(path?.getAttribute("stroke")).toBe(CHART_COLORS.primary);
  });

  it("uses a custom color when provided", () => {
    const { container } = render(
      <LineChart data={SAMPLE_DATA} width={400} height={300} color={CHART_COLORS.danger} />,
    );
    const path = container.querySelector("path");
    expect(path?.getAttribute("stroke")).toBe(CHART_COLORS.danger);
  });

  it("does not render dots by default", () => {
    const { container } = render(<LineChart data={SAMPLE_DATA} width={400} height={300} />);
    const circles = container.querySelectorAll("circle");
    expect(circles).toHaveLength(0);
  });

  it("renders dots at each data point when showDots=true", () => {
    const { container } = render(
      <LineChart data={SAMPLE_DATA} width={400} height={300} showDots />,
    );
    const circles = container.querySelectorAll("circle");
    expect(circles).toHaveLength(SAMPLE_DATA.length);
  });

  it("renders an empty path when data has a single point", () => {
    const { container } = render(<LineChart data={[{ x: 0, y: 10 }]} width={400} height={300} />);
    const path = container.querySelector("path");
    // A single point produces a valid but minimal path (moveTo only)
    expect(path).not.toBeNull();
  });
});
