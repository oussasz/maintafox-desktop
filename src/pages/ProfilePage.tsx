import { Navigate } from "react-router-dom";

/** Legacy route — redirects into Settings → My Profile. */
export function ProfilePage() {
  return <Navigate to="/settings?tab=profile" replace />;
}
