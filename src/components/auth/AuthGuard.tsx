import { useCallback } from "react";
import { Navigate, Outlet } from "react-router-dom";

import { PermissionProvider } from "@/contexts/PermissionContext";
import { useSession } from "@/hooks/use-session";
import { ForcePasswordChangePage } from "@/pages/auth/ForcePasswordChangePage";
import { LockScreen } from "@/pages/auth/LockScreen";
import { logout as authLogout, unlockSessionWithPin } from "@/services/auth-service";

/**
 * AuthGuard: session-state router.
 *
 * Sits between the router root and the ShellLayout. Renders one of:
 * 1. Loading spinner — while session state is being fetched
 * 2. Navigate to /login — if not authenticated and not locked
 * 3. LockScreen — if session is idle-locked (has user but locked)
 * 4. ForcePasswordChangePage — if authenticated but must change password
 * 5. <Outlet /> — normal authenticated state → ShellLayout renders
 *
 * Each sub-screen receives callbacks that trigger a session refresh,
 * causing AuthGuard to re-evaluate and potentially show a different screen.
 */
export function AuthGuard() {
  const session = useSession();

  const handleUnlock = useCallback(
    async (password: string) => {
      await session.unlock(password);
    },
    [session],
  );

  const handleUnlockWithPin = useCallback(
    async (pin: string) => {
      await unlockSessionWithPin({ pin });
      // Force session refresh to pick up the new state
      void session.refresh();
    },
    [session],
  );

  const handleForceChange = useCallback(
    async (newPassword: string) => {
      await session.changePassword(newPassword);
    },
    [session],
  );

  const handleLogout = useCallback(async () => {
    await authLogout();
    // After logout, session.info becomes UNAUTHENTICATED on next render
    await session.refresh();
  }, [session]);

  // 1. Loading
  if (session.isLoading) {
    return (
      <div className="flex h-screen items-center justify-center bg-surface-0">
        <div
          className="h-8 w-8 animate-spin rounded-full border-2
                     border-surface-3 border-t-primary"
        />
      </div>
    );
  }

  const info = session.info;

  // 2. Locked session — show lock screen (before auth check because
  //    is_authenticated is false when locked)
  if (info?.is_locked && info.user_id !== null) {
    return (
      <LockScreen
        displayName={info.display_name ?? info.username}
        onUnlock={handleUnlock}
        onUnlockWithPin={handleUnlockWithPin}
        onLogout={handleLogout}
        pinConfigured={info.pin_configured ?? false}
      />
    );
  }

  // 3. Not authenticated — redirect to login
  if (!info?.is_authenticated) {
    return <Navigate to="/login" replace />;
  }

  // 4. Force password change required
  if (info.force_password_change) {
    return <ForcePasswordChangePage onComplete={handleForceChange} />;
  }

  // 5. Normal authenticated state — wrap with PermissionProvider
  return (
    <PermissionProvider>
      <Outlet />
    </PermissionProvider>
  );
}
