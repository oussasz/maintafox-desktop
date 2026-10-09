import { Navigate } from "react-router-dom";

/** @deprecated Use `/analytics/*` hub routes. Kept so stale imports do not break. */
export function AnalyticsPage() {
  return <Navigate to="/analytics/dashboards" replace />;
}
