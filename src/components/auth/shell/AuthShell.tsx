import { ShieldCheck } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import { mfAuth } from "@/design-system/tokens";
import { useCompactAuthWindow } from "@/hooks/use-compact-auth-window";
import { cn } from "@/lib/utils";

import { AuthLocaleSwitch } from "./AuthPrimitives";

export interface AuthShellProps {
  children: ReactNode;
  /** `sm` ≈ 400 px (sign-in), `md` ≈ 480 px (activation, forms with a summary card). */
  width?: "sm" | "md";
  /** Extra classes on the card (e.g. a scrollable max height). */
  cardClassName?: string;
}

/**
 * Page layout for sign-in and licence activation: an installer-style card with a navy brand band
 * (logo and product name), and a quiet footer (security note + language switch). The window itself
 * is compact and fixed-size while these screens are shown (`useCompactAuthWindow`). The page
 * still scrolls on short windows or large display zoom, and every control is at least 44 px tall.
 */
export function AuthShell({ children, width = "sm", cardClassName }: AuthShellProps) {
  const { t } = useTranslation("auth");
  useCompactAuthWindow();
  return (
    <div className={mfAuth.shell}>
      <main
        className={cn(
          width === "md" ? mfAuth.cardCompact : mfAuth.card,
          "overflow-hidden !p-0",
          cardClassName,
        )}
      >
        <div className={mfAuth.cardBrandBand}>
          <MaintafoxWordmark size="lg" tone="onDark" align="center" />
        </div>
        <div className="p-6 sm:p-8">{children}</div>
      </main>
      <footer
        className={cn(
          "mt-5 flex w-full flex-col items-center gap-3 sm:flex-row sm:justify-between",
          width === "md" ? "max-w-[480px]" : "max-w-[400px]",
        )}
      >
        <span className="inline-flex items-center gap-1.5 text-xs text-text-muted">
          <ShieldCheck className="h-3.5 w-3.5" aria-hidden />
          {t("login.security.label")}
        </span>
        <AuthLocaleSwitch />
      </footer>
    </div>
  );
}
