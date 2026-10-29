// The field app's building blocks, after Apple's Human Interface Guidelines: navigation bar with a
// large title, inset grouped lists, form cells with visible labels, switches, segmented controls,
// bottom sheets, tab bar. Styles in ../mobile.css; every component mirrors for Arabic.

import { Check, ChevronLeft, ChevronRight, Search, X } from "lucide-react";
import {
  type ButtonHTMLAttributes,
  type InputHTMLAttributes,
  type PointerEvent,
  type ReactNode,
  type TextareaHTMLAttributes,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

import { useMobileT } from "../i18n";
import { isTestServer, serverAddress } from "../lib/server";

function cx(...names: Array<string | false | null | undefined>): string {
  return names.filter(Boolean).join(" ");
}

// ── Screen and navigation ───────────────────────────────────────────────

/** Scroll offset after which the large title has scrolled away and the bar shows the inline title. */
const LARGE_TITLE_SCROLL = 36;

function useScrolledPast(offset: number): boolean {
  const [past, setPast] = useState(false);
  useEffect(() => {
    const onScroll = () => setPast(window.scrollY > offset);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, [offset]);
  return past;
}

/** Red "test server" marker on every screen of a build that does not talk to the real server. */
export function TestServerFlag() {
  const { t } = useMobileT();
  if (!isTestServer) return null;
  return (
    <span className="mf-flag mf-flag--test" title={serverAddress}>
      {t("server.testFlag")}
    </span>
  );
}

/** Orange "mockup" marker of a screen whose feature is not built yet (design preview only). */
export function MockupFlag() {
  const { t } = useMobileT();
  return <span className="mf-flag mf-flag--mockup">{t("preview.mockup")}</span>;
}

/**
 * One screen: a translucent navigation bar (back button, inline title once the large title has
 * scrolled away, trailing actions), the large title, then the content.
 */
export function Screen({
  title,
  subtitle,
  inlineTitle = false,
  hideTitle = false,
  leading,
  trailing,
  withTabs = false,
  pushed = false,
  center = false,
  children,
}: {
  title: string;
  subtitle?: ReactNode;
  /** Title in the bar only (detail screens), no large title. */
  inlineTitle?: boolean;
  /** No title in the bar (onboarding screens carry their own hero title). */
  hideTitle?: boolean;
  leading?: ReactNode;
  trailing?: ReactNode;
  withTabs?: boolean;
  pushed?: boolean;
  center?: boolean;
  children: ReactNode;
}) {
  const scrolled = useScrolledPast(LARGE_TITLE_SCROLL);
  const showLarge = !inlineTitle && !hideTitle;
  return (
    <div className={cx("mf-screen", withTabs && "mf-screen--tabs", pushed && "mf-screen--push")}>
      <header
        className={cx(
          "mf-navbar",
          inlineTitle && "mf-navbar--inline",
          showLarge && scrolled && "is-scrolled",
        )}
      >
        <div className="mf-navbar__leading">{leading}</div>
        <div className="mf-navbar__title" aria-hidden={showLarge && !scrolled}>
          {hideTitle ? null : title}
        </div>
        <div className="mf-navbar__trailing">
          <TestServerFlag />
          {trailing}
        </div>
      </header>
      <main className={cx("mf-content", center && "mf-content--center")}>
        {showLarge ? (
          <div>
            <h1 className="mf-large-title">{title}</h1>
            {subtitle ? <p className="mf-large-subtitle">{subtitle}</p> : null}
          </div>
        ) : inlineTitle ? (
          <h1 className="mf-visually-hidden">{title}</h1>
        ) : null}
        {children}
      </main>
    </div>
  );
}

/** "‹ Back" of the navigation bar; the chevron points the other way in Arabic. */
export function BackButton({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button type="button" className="mf-navbtn" onClick={onClick}>
      <ChevronLeft className="mf-mirror" size={26} strokeWidth={2.4} aria-hidden />
      <span>{label}</span>
    </button>
  );
}

export function NavButton({
  label,
  onClick,
  icon,
  bold = false,
  disabled = false,
}: {
  label: string;
  onClick: () => void;
  icon?: ReactNode;
  bold?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      className="mf-navbtn"
      onClick={onClick}
      disabled={disabled}
      aria-label={icon ? label : undefined}
      style={bold ? { fontWeight: 600 } : undefined}
    >
      {icon ?? label}
    </button>
  );
}

// ── Lists ───────────────────────────────────────────────────────────────

/** An inset grouped section: header, the rounded group, footer. */
export function Section({
  header,
  footer,
  list = false,
  label,
  children,
}: {
  header?: ReactNode;
  footer?: ReactNode;
  /** The rows are `<li>` items (a real list). */
  list?: boolean;
  /** Accessible name when there is no visible header. */
  label?: string;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section className="mf-section" aria-labelledby={header ? id : undefined} aria-label={label}>
      {header ? (
        <h2 id={id} className="mf-section__header">
          {header}
        </h2>
      ) : null}
      {list ? <ul className="mf-group">{children}</ul> : <div className="mf-group">{children}</div>}
      {footer ? <p className="mf-section__footer">{footer}</p> : null}
    </section>
  );
}

export type RowTone = "default" | "tint" | "destructive";

/** One row: optional coloured icon tile, title and subtitle, trailing detail, chevron. */
export function Row({
  icon,
  iconColor,
  title,
  subtitle,
  detail,
  accessory,
  chevron = false,
  tone = "default",
  center = false,
  onClick,
  disabled,
  ariaLabel,
}: {
  icon?: ReactNode;
  iconColor?: string;
  title: ReactNode;
  subtitle?: ReactNode;
  detail?: ReactNode;
  accessory?: ReactNode;
  chevron?: boolean;
  tone?: RowTone;
  center?: boolean;
  onClick?: () => void;
  disabled?: boolean;
  ariaLabel?: string;
}) {
  const className = cx(
    "mf-row",
    icon != null && "mf-row--icon",
    tone === "tint" && "mf-row--tint",
    tone === "destructive" && "mf-row--destructive",
    center && "mf-row--center",
  );
  const body = (
    <>
      {icon != null ? (
        <span className="mf-row__icon" style={iconColor ? { background: iconColor } : undefined}>
          {icon}
        </span>
      ) : null}
      <span className={center ? undefined : "mf-row__main"}>
        <span className="mf-row__title">{title}</span>
        {subtitle ? <span className="mf-row__subtitle">{subtitle}</span> : null}
      </span>
      {detail != null ? <span className="mf-row__detail">{detail}</span> : null}
      {accessory}
      {chevron ? (
        <ChevronRight className="mf-row__chevron mf-mirror" size={20} aria-hidden />
      ) : null}
    </>
  );
  if (onClick) {
    return (
      <button
        type="button"
        className={className}
        onClick={onClick}
        disabled={disabled}
        aria-label={ariaLabel}
      >
        {body}
      </button>
    );
  }
  return <div className={className}>{body}</div>;
}

/** A labelled one-line input inside a group (the label stays visible above the value). */
export function TextCell({
  label,
  ...input
}: { label: string } & InputHTMLAttributes<HTMLInputElement>) {
  return (
    <label className="mf-cell">
      <span className="mf-cell__label">{label}</span>
      <input className="mf-cell__input" {...input} />
    </label>
  );
}

/** A labelled multi-line input inside a group, with its length when capped. */
export function TextAreaCell({
  label,
  value,
  maxLength,
  ...area
}: { label: string; value: string } & TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <div className="mf-cell">
      <label className="mf-cell__field">
        <span className="mf-cell__label">{label}</span>
        <textarea className="mf-cell__input" value={value} maxLength={maxLength} {...area} />
      </label>
      {maxLength ? (
        <span className="mf-cell__count" aria-hidden>
          {value.length}/{maxLength}
        </span>
      ) : null}
    </div>
  );
}

/** A row with an iOS switch. */
export function ToggleRow({
  label,
  icon,
  iconColor,
  checked,
  onChange,
}: {
  label: string;
  icon?: ReactNode;
  iconColor?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className={cx("mf-row", icon != null && "mf-row--icon")}>
      {icon != null ? (
        <span className="mf-row__icon" style={iconColor ? { background: iconColor } : undefined}>
          {icon}
        </span>
      ) : null}
      <span className="mf-row__main">{label}</span>
      <input
        type="checkbox"
        role="switch"
        className="mf-switch"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
      />
    </label>
  );
}

export interface PickerOption {
  value: string;
  label: string;
  hint?: string;
  /** A colour dot before the label (urgency scale colours from the reference data). */
  color?: string | null;
}

/** Options from which a picker sheet offers a search field. */
const PICKER_SEARCH_FROM = 9;

/**
 * A row showing the chosen value; tapping it opens a bottom sheet listing the options with a
 * checkmark on the chosen one (iOS list picker). Options come from the caller (the engine's
 * reference data), never from here.
 */
export function PickerRow({
  label,
  options,
  value,
  onChange,
  placeholder,
}: {
  label: string;
  options: PickerOption[];
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  const { t } = useMobileT();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const chosen = options.find((o) => o.value === value) ?? null;
  const needle = query.trim().toLocaleLowerCase();
  const shown = needle
    ? options.filter((o) => o.label.toLocaleLowerCase().includes(needle))
    : options;
  return (
    <>
      <button
        type="button"
        className="mf-row"
        aria-haspopup="dialog"
        aria-label={`${label}: ${chosen ? chosen.label : placeholder}`}
        onClick={() => {
          setQuery("");
          setOpen(true);
        }}
      >
        <span className="mf-row__main">
          <span className="mf-cell__label">{label}</span>
          <span
            className="mf-row__title"
            style={chosen ? undefined : { color: "var(--mf-label-3)" }}
          >
            {chosen?.color ? <Dot color={chosen.color} /> : null}
            {chosen ? chosen.label : placeholder}
          </span>
        </span>
        <ChevronRight className="mf-row__chevron mf-mirror" size={20} aria-hidden />
      </button>
      <Sheet
        open={open}
        title={label}
        onClose={() => setOpen(false)}
        closeLabel={t("common.close")}
      >
        {options.length >= PICKER_SEARCH_FROM ? (
          <SearchField value={query} onChange={setQuery} placeholder={t("common.search")} />
        ) : null}
        <Section list label={label}>
          {shown.map((o) => (
            <li key={o.value}>
              <button
                type="button"
                role="radio"
                aria-checked={o.value === value}
                className="mf-row"
                onClick={() => {
                  onChange(o.value);
                  setOpen(false);
                }}
              >
                <span className="mf-row__main">
                  <span className="mf-row__title">
                    {o.color ? <Dot color={o.color} /> : null}
                    {o.label}
                  </span>
                  {o.hint ? <span className="mf-row__subtitle">{o.hint}</span> : null}
                </span>
                {o.value === value ? (
                  <Check size={22} strokeWidth={2.6} color="var(--mf-tint)" aria-hidden />
                ) : null}
              </button>
            </li>
          ))}
        </Section>
      </Sheet>
    </>
  );
}

function Dot({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      style={{
        display: "inline-block",
        inlineSize: 10,
        blockSize: 10,
        borderRadius: "50%",
        background: color,
        marginInline: 8,
        verticalAlign: "middle",
      }}
    />
  );
}

export function SearchField({
  value,
  onChange,
  placeholder,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
}) {
  return (
    <div className="mf-search">
      <Search size={18} aria-hidden />
      <input
        type="search"
        value={value}
        placeholder={placeholder}
        aria-label={placeholder}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}

// ── Controls ────────────────────────────────────────────────────────────

export type ButtonVariant =
  | "filled"
  | "tinted"
  | "gray"
  | "plain"
  | "destructive"
  | "go"
  | "attention";

export function Button({
  variant = "filled",
  className,
  type = "button",
  ...rest
}: { variant?: ButtonVariant } & ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button type={type} className={cx("mf-btn", `mf-btn--${variant}`, className)} {...rest} />;
}

export function Actions({ row = false, children }: { row?: boolean; children: ReactNode }) {
  return <div className={cx("mf-actions", row && "mf-actions--row")}>{children}</div>;
}

/** iOS segmented control; `aria-pressed` marks the chosen segment. */
export function Segmented<V extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: Array<{ value: V; label: string; lang?: string }>;
  value: V;
  onChange: (value: V) => void;
}) {
  return (
    <div className="mf-segmented" role="group" aria-label={label}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          className="mf-segmented__btn"
          aria-pressed={o.value === value}
          lang={o.lang}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

// ── Status ──────────────────────────────────────────────────────────────

export type Tone = "neutral" | "tint" | "green" | "orange" | "red";

export function Pill({
  tone = "neutral",
  dot = false,
  dotColor,
  children,
}: {
  tone?: Tone;
  dot?: boolean;
  /** Dot colour given by the engine (status or urgency colour of the reference data). */
  dotColor?: string | null | undefined;
  children: ReactNode;
}) {
  return (
    <span className={cx("mf-pill", tone !== "neutral" && `mf-pill--${tone}`)}>
      {dot || dotColor ? (
        <span
          className="mf-pill__dot"
          style={dotColor ? { background: dotColor } : undefined}
          aria-hidden
        />
      ) : null}
      {children}
    </span>
  );
}

export type BannerTone = "info" | "success" | "warning" | "danger" | "safety";

export function Banner({
  tone,
  icon,
  title,
  children,
  alert = false,
}: {
  tone: BannerTone;
  icon?: ReactNode;
  title?: ReactNode;
  children?: ReactNode;
  /** Announced at once (refusals, errors). */
  alert?: boolean;
}) {
  return (
    <div className={cx("mf-banner", `mf-banner--${tone}`)} role={alert ? "alert" : undefined}>
      {icon ? <span className="mf-banner__icon">{icon}</span> : null}
      <div className="mf-banner__body">
        {title ? <p className="mf-banner__title">{title}</p> : null}
        {children ? <div className="mf-banner__text">{children}</div> : null}
      </div>
    </div>
  );
}

export function EmptyState({
  icon,
  tone = "tint",
  title,
  text,
  children,
}: {
  icon: ReactNode;
  tone?: "tint" | "red" | "orange" | "green";
  title: string;
  text?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="mf-empty">
      <span className={cx("mf-empty__icon", tone !== "tint" && `mf-empty__icon--${tone}`)}>
        {icon}
      </span>
      <h2 className="mf-empty__title">{title}</h2>
      {text ? <p className="mf-empty__text">{text}</p> : null}
      {children}
    </div>
  );
}

const SPOKES = 8;

/** The iOS activity indicator. */
export function Spinner({ large = false }: { large?: boolean }) {
  return (
    <span className={cx("mf-spinner", large && "mf-spinner--lg")} aria-hidden>
      {Array.from({ length: SPOKES }, (_, i) => (
        <span
          key={i}
          style={{
            transform: `rotate(${(360 / SPOKES) * i}deg)`,
            animationDelay: `${-1 + i / SPOKES}s`,
          }}
        />
      ))}
    </span>
  );
}

/** A spinner (or an indeterminate bar) with what is happening, announced politely. */
export function Progress({ text, bar = false }: { text: ReactNode; bar?: boolean }) {
  return (
    <div className="mf-progress">
      {bar ? (
        <div className="mf-bar" aria-hidden>
          <div className="mf-bar__fill" />
        </div>
      ) : (
        <Spinner large />
      )}
      <p role="status" style={{ margin: 0 }}>
        {text}
      </p>
    </div>
  );
}

/** Onboarding header: the Maintafox mark (or an icon tile), a title and a sentence. */
export function Hero({
  icon,
  tone = "tint",
  title,
  text,
}: {
  /** No icon: the Maintafox logo. */
  icon?: ReactNode;
  tone?: "tint" | "red";
  title: string;
  text?: ReactNode;
}) {
  return (
    <div className="mf-hero">
      {icon ? (
        <span
          className={cx(
            "mf-hero__mark",
            tone === "red" ? "mf-hero__mark--red" : "mf-hero__mark--icon",
          )}
        >
          {icon}
        </span>
      ) : (
        <span className="mf-hero__mark">
          <img src="/icons/maintafox.svg" alt="" />
        </span>
      )}
      <h1 className="mf-hero__title">{title}</h1>
      {text ? <p className="mf-hero__text">{text}</p> : null}
    </div>
  );
}

/** Onboarding progress: licence, plant, sign-in. */
export function Steps({
  count,
  current,
  label,
}: {
  count: number;
  current: number;
  label: string;
}) {
  return (
    <ol className="mf-steps" aria-label={label}>
      {Array.from({ length: count }, (_, i) => (
        <li
          key={i}
          className={i < current ? "is-done" : undefined}
          aria-current={i === current ? "step" : undefined}
        />
      ))}
    </ol>
  );
}

export interface TabItem<K extends string> {
  key: K;
  label: string;
  icon: ReactNode;
  badge?: number;
}

export function TabBar<K extends string>({
  items,
  active,
  onSelect,
  label,
}: {
  items: Array<TabItem<K>>;
  active: K;
  onSelect: (key: K) => void;
  label: string;
}) {
  return (
    <nav className="mf-tabbar" aria-label={label}>
      {items.map((item) => (
        <button
          key={item.key}
          type="button"
          className="mf-tab"
          aria-current={item.key === active ? "page" : undefined}
          onClick={() => onSelect(item.key)}
        >
          {item.icon}
          <span>{item.label}</span>
          {item.badge ? <span className="mf-tab__badge">{item.badge}</span> : null}
        </button>
      ))}
    </nav>
  );
}

/** A short confirmation over the content (e.g. "Request DI-0042 sent"), gone after a few seconds. */
export function Toast({
  children,
  onDone,
  ms = 3200,
}: {
  children: ReactNode;
  onDone: () => void;
  ms?: number;
}) {
  useEffect(() => {
    const timer = setTimeout(onDone, ms);
    return () => clearTimeout(timer);
  }, [onDone, ms]);
  return (
    <div className="mf-toast" role="status">
      {children}
    </div>
  );
}

// ── Bottom sheet ────────────────────────────────────────────────────────

/** A modal bottom sheet: grabber, title bar with a close button, scrollable body. */
export function Sheet({
  open,
  title,
  onClose,
  closeLabel,
  trailing,
  action = false,
  children,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  closeLabel: string;
  trailing?: ReactNode;
  /** iOS action sheet: no close button, the caller's buttons (one of them Cancel) end it. */
  action?: boolean;
  children: ReactNode;
}) {
  const panel = useRef<HTMLDivElement>(null);
  const drag = useRef<{ y: number; dy: number } | null>(null);
  const titleId = useId();
  // The caller's onClose is often a new function each render (e.g. while typing in a search field
  // inside the sheet): read it through a ref so the open/close effect runs on `open` only.
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement as HTMLElement | null;
    panel.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
    };
    document.addEventListener("keydown", onKey);
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    // The screen behind the sheet can be neither tapped nor reached by TalkBack or the keyboard.
    const app = document.getElementById("root");
    const wasInert = app?.hasAttribute("inert") ?? false;
    app?.setAttribute("inert", "");
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = overflow;
      if (!wasInert) app?.removeAttribute("inert");
      previous?.focus?.();
    };
  }, [open]);

  // Swipe the grabber or title bar down to dismiss (past a quarter of the sheet, or 120 px).
  const onDragStart = (e: PointerEvent<HTMLElement>) => {
    drag.current = { y: e.clientY, dy: 0 };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const onDragMove = (e: PointerEvent<HTMLElement>) => {
    if (!drag.current || !panel.current) return;
    drag.current.dy = Math.max(0, e.clientY - drag.current.y);
    panel.current.style.transition = "none";
    panel.current.style.transform = `translateY(${drag.current.dy}px)`;
  };
  const onDragEnd = () => {
    const sheet = panel.current;
    const dy = drag.current?.dy ?? 0;
    drag.current = null;
    if (!sheet) return;
    sheet.style.transition = "transform 260ms var(--mf-ease)";
    if (dy > Math.min(120, sheet.offsetHeight / 4)) {
      sheet.style.transform = "translateY(100%)";
      setTimeout(() => closeRef.current(), 200);
    } else {
      sheet.style.transform = "";
    }
  };
  const dragHandlers = {
    onPointerDown: onDragStart,
    onPointerMove: onDragMove,
    onPointerUp: onDragEnd,
    onPointerCancel: onDragEnd,
  };

  if (!open) return null;
  return createPortal(
    <div className="mf-sheet-root">
      <button
        type="button"
        className="mf-sheet-scrim"
        aria-label={closeLabel}
        tabIndex={-1}
        onClick={onClose}
      />
      <div
        ref={panel}
        className="mf-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
      >
        <span className="mf-sheet__grabber" aria-hidden {...dragHandlers} />
        <div className="mf-sheet__head" {...dragHandlers}>
          <div>
            {action ? null : (
              <NavButton label={closeLabel} onClick={onClose} icon={<X size={24} aria-hidden />} />
            )}
          </div>
          <h2 id={titleId} className="mf-sheet__title">
            {title}
          </h2>
          <div className="mf-sheet__trailing">{trailing}</div>
        </div>
        <div className="mf-sheet__body">{children}</div>
      </div>
    </div>,
    document.body,
  );
}
