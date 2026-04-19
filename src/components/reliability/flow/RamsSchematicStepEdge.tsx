import { BaseEdge, type EdgeProps, getSmoothStepPath } from "@xyflow/react";

/**
 * Orthogonal “schematic” wiring with a larger first/last segment offset so
 * traces read more like electrical CAD than compact flowcharts.
 */
export function RamsSchematicStepEdge(props: EdgeProps) {
  const {
    id,
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    markerEnd,
    style,
  } = props;
  const [path] = getSmoothStepPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
    borderRadius: 0,
    /** Tight first/last segments so orthogonals meet handles without visible air gap. */
    offset: 14,
  });
  return (
    <BaseEdge
      id={id}
      path={path}
      style={{
        ...style,
        strokeWidth: 1,
        strokeLinecap: "square",
        strokeLinejoin: "miter",
        shapeRendering: "crispEdges",
      }}
      interactionWidth={14}
      {...(markerEnd != null && markerEnd !== "" ? { markerEnd } : {})}
    />
  );
}
