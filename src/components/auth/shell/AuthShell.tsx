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
 * Page layout for sign-in and licence activation: the window itself is the screen. A navy brand
 * band (logo and product name) runs edge to edge under the title bar, the form follows, and a quiet
 * footer (security note + language switch) closes the same surface: no inner card or margin. The
 * window starts compact (and can be resized or maximised) while these screens are shown
 * (`useCompactAuthWindow`). The page still scrolls on short windows or large display zoom, and
 * every control is at least 44 px tall.
 */
export function AuthShell({ children, width = "sm", cardClassName }: AuthShellProps) {
  const { t } = useTranslation("auth");
  useCompactAuthWindow();
  const column = width === "md" ? "max-w-[480px]" : "max-w-[400px]";
  return (
    <div className={mfAuth.windowShell}>
      <div className={mfAuth.cardBrandBand}>
        <MaintafoxWordmark size="lg" tone="onDark" align="center" />
      </div>
      <main className={cn("mx-auto w-full flex-1 px-6 py-6 sm:px-8", column, cardClassName)}>
        {children}
      </main>
      <footer
        className={cn(
          "mx-auto flex w-full flex-col items-center gap-3 px-6 pb-5 sm:flex-row sm:justify-between sm:px-8",
          column,
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
