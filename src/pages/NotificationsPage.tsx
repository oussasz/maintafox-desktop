import { Navigate } from "react-router-dom";

/** Legacy route — redirects into Settings → Notifications. */
export function NotificationsPage() {
  return <Navigate to="/settings?tab=notifications" replace />;
}
