import { useCallback } from "react";
import { Link, Navigate, Outlet } from "react-router-dom";

import { PermissionProvider } from "@/contexts/PermissionContext";
import { useSession } from "@/hooks/use-session";
import { clearPermissionCache } from "@/lib/permission-cache";
import { ForcePasswordChangePage } from "@/pages/auth/ForcePasswordChangePage";
import { LockScreen } from "@/pages/auth/LockScreen";
import { logout as authLogout, unlockSessionWithPin } from "@/services/auth-service";
import { useAuthInterceptorStore } from "@/store/auth-interceptor-store";

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
 * PermissionProvider is kept on a single branch for authenticated shell and
 * auth-interceptor shell preservation so a lock transition does not remount
 * it and wipe in-memory permissions.
 */
export function AuthGuard() {
  const session = useSession();
  const isBootstrapping = session.isLoading && session.info === null;
  const isAuthLockOpen = useAuthInterceptorStore((s) => s.isLockOpen);

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
    clearPermissionCache();
    // After logout, session.info becomes UNAUTHENTICATED on next render
    await session.refresh();
  }, [session]);

  // 1. Loading
  if (isBootstrapping) {
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

  // 3. Not authenticated and no interceptor shell — redirect to login
  if (!info?.is_authenticated && !isAuthLockOpen) {
    return <Navigate to="/login" replace />;
  }

  // 4. Force password change required (authenticated only)
  if (info?.is_authenticated && info.force_password_change) {
    return <ForcePasswordChangePage onComplete={handleForceChange} />;
  }

  const warnDays = info?.password_expires_in_days;
  const showPasswordWarning =
    info?.is_authenticated === true && typeof warnDays === "number" && warnDays <= 14;

  // 5. Authenticated shell, or interceptor preserving shell without a session.
  //    One PermissionProvider branch so lock/auth flips do not remount and wipe perms.
  return (
    <PermissionProvider>
      <>
        {showPasswordWarning && (
          <div className="sticky top-0 z-40 border-b border-amber-300 bg-amber-50 px-4 py-2 text-amber-900">
            <div className="mx-auto flex max-w-7xl flex-wrap items-center justify-between gap-3 text-sm">
              <p>
                Votre mot de passe expire dans {warnDays} {warnDays <= 1 ? "jour" : "jours"}.
              </p>
              <Link
                to="/profile"
                className="font-semibold underline decoration-amber-700 underline-offset-2"
              >
                Changer maintenant
              </Link>
            </div>
          </div>
        )}
        <Outlet />
      </>
    </PermissionProvider>
  );
}
