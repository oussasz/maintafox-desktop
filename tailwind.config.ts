import type { Config } from "tailwindcss";

/**
 * Colours defined as CSS variables cannot take Tailwind's `/<alpha>` modifier on their own (on
 * Tailwind 3 `bg-muted/50`, `bg-tone-danger-bg/40` … generated no CSS at all). Each variable colour
 * becomes a function: unmodified it stays `var(--x)`; with a modifier it is mixed with transparent
 * (`color-mix`, supported by WebView2), so the theme switch keeps working.
 */
type ColorFn = (opts: { opacityValue?: string }) => string;
type ColorTree = { [key: string]: string | ColorFn | ColorTree };

export function cssVarColor(value: string): ColorFn {
  return ({ opacityValue }) => {
    if (opacityValue === undefined || opacityValue === "1" || !/^[0-9.]+$/.test(opacityValue)) {
      return value;
    }
    const pct = Math.round(Number(opacityValue) * 1000) / 10;
    return `color-mix(in srgb, ${value} ${pct}%, transparent)`;
  };
}

function withAlpha(tree: ColorTree): ColorTree {
  const out: ColorTree = {};
  for (const [key, value] of Object.entries(tree)) {
    if (typeof value === "string") out[key] = value.startsWith("var(") ? cssVarColor(value) : value;
    else if (typeof value === "function") out[key] = value;
    else out[key] = withAlpha(value);
  }
  return out;
}

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: withAlpha({
        /* ── Shadcn/ui semantic tokens (resolve via CSS custom properties) ── */
        background: "var(--background)",
        foreground: "var(--foreground)",
        card: {
          DEFAULT: "var(--card)",
          foreground: "var(--card-foreground)",
        },
        popover: {
          DEFAULT: "var(--popover)",
          foreground: "var(--popover-foreground)",
        },
        secondary: {
          DEFAULT: "var(--secondary)",
          foreground: "var(--secondary-foreground)",
        },
        muted: {
          DEFAULT: "var(--muted)",
          foreground: "var(--muted-foreground)",
        },
        destructive: {
          DEFAULT: "var(--destructive)",
          foreground: "var(--destructive-foreground)",
        },
        border: "var(--border)",
        input: "var(--input)",
        ring: "var(--ring)",
        /* ── Maintafox brand tokens ─────────────────────────────────────── */
        primary: {
          DEFAULT: "#0049ac",
          dark: "#003a8c",
          light: "#4d7fd0",
          bg: "#e6eefb",
          foreground: "var(--primary-foreground)",
        },
        /** Logo orange — alerts and the single primary attention point of a screen. */
        accent: {
          DEFAULT: "#f68714",
          dark: "#e8780a",
          foreground: "var(--accent-foreground)",
        },
        /** Brand identity colours (sidebar, charts, print). Literal hex so opacity modifiers work;
         keep in sync with --color-* in globals.css. Prefer semantic tokens in UI. */
        brand: {
          navy: "#0b1f3a",
          primary: "#0049ac",
          orange: "#f68714",
        },
        /** Status tones — see `mfTone` in src/design-system/tokens.ts. */
        tone: {
          neutral: {
            bg: "var(--tone-neutral-bg)",
            fg: "var(--tone-neutral-fg)",
            border: "var(--tone-neutral-border)",
          },
          info: {
            bg: "var(--tone-info-bg)",
            fg: "var(--tone-info-fg)",
            border: "var(--tone-info-border)",
          },
          progress: {
            bg: "var(--tone-progress-bg)",
            fg: "var(--tone-progress-fg)",
            border: "var(--tone-progress-border)",
          },
          attention: {
            bg: "var(--tone-attention-bg)",
            fg: "var(--tone-attention-fg)",
            border: "var(--tone-attention-border)",
          },
          warning: {
            bg: "var(--tone-warning-bg)",
            fg: "var(--tone-warning-fg)",
            border: "var(--tone-warning-border)",
          },
          success: {
            bg: "var(--tone-success-bg)",
            fg: "var(--tone-success-fg)",
            border: "var(--tone-success-border)",
          },
          danger: {
            bg: "var(--tone-danger-bg)",
            fg: "var(--tone-danger-fg)",
            border: "var(--tone-danger-border)",
          },
        },
        status: {
          success: "#198754",
          danger: "#dc3545",
          warning: "#ffc107",
          info: "#0dcaf0",
          neutral: "#6c757d",
        },
        surface: {
          0: "var(--surface-0)",
          1: "var(--surface-1)",
          2: "var(--surface-2)",
          3: "var(--surface-3)",
          border: "var(--surface-border)",
        },
        text: {
          primary: "var(--text-primary)",
          secondary: "var(--text-secondary)",
          muted: "var(--text-muted)",
          danger: "var(--text-danger)",
          success: "var(--text-success)",
          warning: "var(--text-warning)",
        },
        // Colour functions are supported by Tailwind 3 but not described by its types.
      }) as unknown as Record<string, string>,
      fontFamily: {
        sans: [
          "Inter",
          "ui-sans-serif",
          "system-ui",
          "-apple-system",
          "BlinkMacSystemFont",
          '"Segoe UI"',
          "Roboto",
          "sans-serif",
        ],
        mono: ['"JetBrains Mono"', '"Fira Code"', "ui-monospace", "monospace"],
      },
      fontSize: {
        "2xs": ["0.625rem", { lineHeight: "1rem" }],
        xs: ["0.75rem", { lineHeight: "1rem" }],
        sm: ["0.8125rem", { lineHeight: "1.25rem" }],
        base: ["0.875rem", { lineHeight: "1.5rem" }],
        lg: ["1rem", { lineHeight: "1.5rem" }],
        xl: ["1.125rem", { lineHeight: "1.75rem" }],
        "2xl": ["1.25rem", { lineHeight: "1.75rem" }],
      },
      spacing: {
        sidebar: "240px",
        "sidebar-sm": "64px",
        topbar: "52px",
        statusbar: "24px",
      },
      animation: {
        "fade-in": "fadeIn 120ms ease-in",
        "slide-in": "slideIn 150ms ease-out",
        "spin-slow": "spin 2s linear infinite",
      },
      keyframes: {
        fadeIn: { from: { opacity: "0" }, to: { opacity: "1" } },
        slideIn: {
          from: { transform: "translateX(-8px)", opacity: "0" },
          to: { transform: "translateX(0)", opacity: "1" },
        },
      },
      borderRadius: {
        sm: "4px",
        md: "6px",
        lg: "8px",
        xl: "12px",
      },
      borderColor: {
        DEFAULT: "var(--border)",
      },
      boxShadow: {
        card: "var(--shadow-card)",
        panel: "var(--shadow-panel)",
        focus: "var(--shadow-focus)",
      },
      transitionDuration: {
        fast: "100ms",
        base: "150ms",
        normal: "200ms",
        slow: "300ms",
      },
    },
  },
  plugins: [require("@tailwindcss/forms"), require("@tailwindcss/typography")],
} satisfies Config;
