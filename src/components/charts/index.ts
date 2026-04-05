// Barrel export for chart primitives.
// Module code imports from "@/components/charts" — never from individual files.

export { BarChart } from "./BarChart";
export type { BarChartDatum, BarChartProps } from "./BarChart";
export { LineChart } from "./LineChart";
export type { LineChartDatum, LineChartProps } from "./LineChart";
export {
  CHART_COLORS,
  CHART_PALETTE,
  DEFAULT_MARGIN,
  getInnerDimensions,
  useContainerSize,
} from "./chart-utils";
export type { ChartMargin, InnerDimensions } from "./chart-utils";
