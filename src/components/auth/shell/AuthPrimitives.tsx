import { AlertTriangle, CheckCircle2, Info, Loader2, XCircle } from "lucide-react";
import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";

import { mfAuth, mfAuthAlert } from "@/design-system/tokens";
import { i18n } from "@/i18n";
import { cn } from "@/lib/utils";
import { useLocaleStore } from "@/stores/locale-store";

/* ── Buttons ────────────────────────────────────────────────────────────────── */

export type AuthButtonVariant = "primary" | "secondary" | "ghost";

const BUTTON_CLASS: Record<AuthButtonVariant, string> = {
  primary: mfAuth.buttonPrimary,
  secondary: mfAuth.buttonSecondary,
  ghost: mfAuth.buttonGhost,
};

export interface AuthButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: AuthButtonVariant;
  /** Shows a spinner, keeps the label, and blocks further clicks. */
  loading?: boolean;
}

/**
 * Authentication-screen button. Primary = logo orange (one per screen), secondary = blue outline,
 * ghost = text action. Focus ring comes from `.mf-auth` (styles/auth.css).
 */
export const AuthButton = forwardRef<HTMLButtonElement, AuthButtonProps>(function AuthButton(
  { variant = "primary", loading = false, disabled, className, children, type = "button", ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={cn(BUTTON_CLASS[variant], className)}
      {...rest}
    >
      {loading ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : null}
      {children}
    </button>
  );
});

/* ── Alerts ─────────────────────────────────────────────────────────────────── */

export type AuthAlertTone = "info" | "warning" | "danger" | "success";

const ALERT_ICON: Record<AuthAlertTone, { icon: typeof Info; className: string }> = {
  info: { icon: Info, className: "text-[var(--mfa-blue)]" },
  warning: { icon: AlertTriangle, className: "text-[var(--mfa-orange-dark)]" },
  danger: { icon: XCircle, className: "text-[var(--mfa-danger)]" },
  success: { icon: CheckCircle2, className: "text-[var(--mfa-success)]" },
};

export interface AuthAlertProps {
  tone: AuthAlertTone;
  title?: ReactNode;
  children?: ReactNode;
  className?: string;
  /** Overrides the default role (`alert` for warning/danger, `status` for info/success). */
  role?: "alert" | "status";
}

/** Icon + text callout. Warnings and errors are announced to assistive technology immediately. */
export function AuthAlert({ tone, title, children, className, role }: AuthAlertProps) {
  const { icon: Icon, className: iconClass } = ALERT_ICON[tone];
  const resolvedRole = role ?? (tone === "danger" || tone === "warning" ? "alert" : "status");
  return (
    <div
      role={resolvedRole}
      className={cn("flex items-start gap-2.5", mfAuthAlert[tone], className)}
    >
      <Icon className={cn("mt-0.5 h-4 w-4 shrink-0", iconClass)} aria-hidden />
      <div className="min-w-0 flex-1 space-y-1">
        {title ? <p className="font-semibold leading-snug">{title}</p> : null}
        {children}
      </div>
    </div>
  );
}

/* ── Language switch ────────────────────────────────────────────────────────── */

/** Compact segmented FR | EN control. Persists through the shared locale store. */
export function AuthLocaleSwitch({ className }: { className?: string }) {
  const { activeLocale, supportedLocales } = useLocaleStore();
  return (
    <div
      role="group"
      aria-label="Language"
      className={cn(
        "inline-flex rounded-lg border border-surface-border bg-white p-0.5 text-xs font-semibold",
        className,
      )}
    >
      {supportedLocales.map((loc) => {
        const active = loc === activeLocale;
        return (
          <button
            key={loc}
            type="button"
            aria-pressed={active}
            onClick={() => {
              void i18n.changeLanguage(loc);
              useLocaleStore.setState({ activeLocale: loc });
            }}
            className={cn(
              "min-h-9 min-w-10 rounded-md px-3 py-1.5 transition-colors",
              active
                ? "bg-[var(--mfa-blue)] text-white"
                : "text-text-secondary hover:bg-[var(--mfa-blue-tint)] hover:text-text-primary",
            )}
          >
            {loc.toUpperCase()}
          </button>
        );
      })}
    </div>
  );
}

/* ── Status chip ────────────────────────────────────────────────────────────── */

export interface AuthStatusChipProps {
  tone: "ok" | "warn" | "neutral";
  children: ReactNode;
  className?: string;
}

const CHIP_TONE: Record<AuthStatusChipProps["tone"], { wrap: string; dot: string }> = {
  ok: {
    wrap: "bg-[var(--mfa-success-tint)] text-[var(--mfa-success)]",
    dot: "bg-[var(--mfa-success)]",
  },
  warn: {
    wrap: "bg-[var(--mfa-orange-tint)] text-[var(--text-warning)]",
    dot: "bg-[var(--mfa-orange)]",
  },
  neutral: { wrap: "bg-surface-3 text-text-secondary", dot: "bg-text-muted" },
};

/** Small status pill with a colour dot (connectivity, licence state). */
export function AuthStatusChip({ tone, children, className }: AuthStatusChipProps) {
  const palette = CHIP_TONE[tone];
  return (
    <span
      className={cn(
        "inline-flex max-w-full items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium",
        palette.wrap,
        className,
      )}
    >
      <span className={cn("h-1.5 w-1.5 shrink-0 rounded-full", palette.dot)} aria-hidden />
      <span className="truncate">{children}</span>
    </span>
  );
}
