/**
 * Building blocks of the home role panels (OT/DI design system: white cards on the icy ground,
 * grey figures, colour only for an exception, orange only for what the reader must act on).
 *
 * - `HomePanel`: a role panel — title, period selector, refresh; its own loading, error (with
 *   retry) and content states, so one failing panel never blanks the page.
 * - `HomeSection`: one module inside a panel, with the definitions of its figures behind an
 *   info button and a link to the module.
 * - `HomeFigure`: one figure; opens the list behind it when that list exists.
 * - `HomeRecordList`: the first records behind a figure, each opening the record itself.
 */
import { ChevronRight, Info, type LucideIcon, RefreshCw } from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { type Tone, mfTone } from "@/design-system/tone";
import { cn } from "@/lib/utils";
import { HOME_PERIODS, type HomePeriod } from "@/services/home-service";

export interface PanelLoad<T> {
  data: T | null;
  loading: boolean;
  error: unknown;
  reload: () => void;
}

/**
 * Loads a panel's one backend call; keeps the last figures while reloading. A change of
 * `refreshSignal` (the page's periodic or event refresh) reloads it.
 */
export function usePanelLoad<T>(load: () => Promise<T>, refreshSignal = 0): PanelLoad<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const seq = useRef(0);

  const reload = useCallback(() => {
    const mine = ++seq.current;
    setLoading(true);
    setError(null);
    load()
      .then((d) => {
        if (mine === seq.current) setData(d);
      })
      .catch((e: unknown) => {
        if (mine === seq.current) setError(e);
      })
      .finally(() => {
        if (mine === seq.current) setLoading(false);
      });
  }, [load]);

  useEffect(() => {
    reload();
  }, [reload, refreshSignal]);

  return { data, loading, error, reload };
}

export function HomePanel(props: {
  icon: LucideIcon;
  title: string;
  subtitle?: string;
  period?: HomePeriod;
  onPeriodChange?: (p: HomePeriod) => void;
  state: PanelLoad<unknown>;
  children: ReactNode;
  testId?: string;
}) {
  const { t } = useTranslation("dashboard");
  const { icon: Icon, title, subtitle, period, onPeriodChange, state } = props;
  return (
    <section
      className="rounded-lg border border-surface-border bg-surface-1 shadow-sm"
      aria-labelledby={`${props.testId ?? title}-title`}
      data-testid={props.testId}
    >
      <header className="flex flex-wrap items-center gap-3 border-b border-surface-border px-4 py-3">
        <Icon className="h-5 w-5 shrink-0 text-primary" aria-hidden />
        <div className="min-w-0 flex-1">
          <h2
            id={`${props.testId ?? title}-title`}
            className="text-base font-semibold text-text-primary"
          >
            {title}
          </h2>
          {subtitle ? <p className="text-xs text-text-muted">{subtitle}</p> : null}
        </div>
        {period !== undefined && onPeriodChange ? (
          <div role="radiogroup" aria-label={t("home.period.label")} className="flex gap-1">
            {HOME_PERIODS.map((p) => (
              <Button
                key={p}
                type="button"
                size="sm"
                role="radio"
                aria-checked={period === p}
                variant={period === p ? "default" : "outline"}
                className="h-9 min-w-[44px]"
                onClick={() => onPeriodChange(p)}
              >
                {t("home.period.days", { count: p })}
              </Button>
            ))}
          </div>
        ) : null}
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-11 w-11 text-text-muted"
          onClick={state.reload}
          disabled={state.loading}
          aria-label={t("home.refresh", { title })}
          title={t("home.refresh", { title })}
        >
          <RefreshCw className={cn("h-4 w-4", state.loading && "animate-spin")} aria-hidden />
        </Button>
      </header>
      <div className="p-4">
        {state.error != null && state.data == null ? (
          <ErrorState error={state.error} onRetry={state.reload} retrying={state.loading} />
        ) : state.data == null ? (
          <LoadingState />
        ) : (
          <>
            {state.error != null ? (
              <ErrorState
                className="mb-4 py-4"
                error={state.error}
                title={t("home.staleTitle")}
                onRetry={state.reload}
                retrying={state.loading}
              />
            ) : null}
            {props.children}
          </>
        )}
      </div>
    </section>
  );
}

export interface FigureDefinition {
  label: string;
  text: string;
}

export function HomeSection(props: {
  title: string;
  definitions: FigureDefinition[];
  openLabel?: string;
  openTo?: string;
  children: ReactNode;
  className?: string;
}) {
  const { t } = useTranslation("dashboard");
  const navigate = useNavigate();
  return (
    <div className={cn("space-y-3", props.className)}>
      <div className="flex items-center gap-2">
        <h3 className="flex-1 text-sm font-semibold text-text-primary">{props.title}</h3>
        <Popover>
          <PopoverTrigger asChild>
            <Button
              type="button"
              variant="ghost"
              size="icon"
              className="h-9 w-9 text-text-muted"
              aria-label={t("home.definitionsAria", { title: props.title })}
            >
              <Info className="h-4 w-4" aria-hidden />
            </Button>
          </PopoverTrigger>
          <PopoverContent align="end" className="w-80 text-sm">
            <p className="font-medium text-text-primary">{props.title}</p>
            <dl className="mt-2 space-y-2 text-xs">
              {props.definitions.map((d) => (
                <div key={d.label}>
                  <dt className="font-medium text-text-secondary">{d.label}</dt>
                  <dd className="text-text-muted">{d.text}</dd>
                </div>
              ))}
            </dl>
          </PopoverContent>
        </Popover>
        {props.openTo && props.openLabel ? (
          <Button
            type="button"
            variant="link"
            className="h-9 px-1 text-xs"
            onClick={() => void navigate(props.openTo as string)}
          >
            {props.openLabel}
            <ChevronRight className="ml-0.5 h-3.5 w-3.5" aria-hidden />
          </Button>
        ) : null}
      </div>
      {props.children}
    </div>
  );
}

/** One figure. `tone` only for an exception (overdue > 0 …); `to` opens the list behind it. */
export function HomeFigure(props: {
  label: string;
  value: string;
  hint?: string;
  tone?: Tone | undefined;
  to?: string | undefined;
}) {
  const { t } = useTranslation("common");
  const navigate = useNavigate();
  const { tone } = props;
  const body = (
    <>
      <span className="text-xs font-medium uppercase tracking-wide text-text-muted">
        {props.label}
      </span>
      <span
        className={cn(
          "mt-1 block font-mono text-2xl font-semibold tabular-nums leading-none",
          tone ? mfTone[tone].text : "text-text-primary",
        )}
      >
        {props.value}
      </span>
      {props.hint || props.to ? (
        <span className="mt-1.5 flex items-center justify-between gap-2 text-xs text-text-muted">
          <span className="truncate">{props.hint}</span>
          {props.to ? <ChevronRight className="h-3.5 w-3.5 shrink-0" aria-hidden /> : null}
        </span>
      ) : null}
    </>
  );
  const frame = cn(
    "flex flex-col rounded-md border bg-surface-1 p-3 text-start",
    tone && tone !== "neutral" ? mfTone[tone].panel : "border-surface-border",
  );
  if (props.to) {
    const to = props.to;
    return (
      <button
        type="button"
        onClick={() => void navigate(to)}
        aria-label={`${props.label}: ${props.value}. ${t("ds.kpi.open")}`}
        className={cn(
          frame,
          "min-h-[44px] w-full transition-colors hover:border-primary/40 hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        )}
      >
        {body}
      </button>
    );
  }
  return <div className={frame}>{body}</div>;
}

export interface RecordRow {
  key: string | number;
  primary: string;
  secondary?: string | undefined;
  meta?: string | undefined;
  tone?: Tone | undefined;
  badge?: string | undefined;
  to: string;
}

/**
 * The first records behind a figure; each row opens the record. With `total`, says how many more
 * there are (never a silent cap).
 */
export function HomeRecordList(props: {
  rows: RecordRow[];
  emptyText: string;
  ariaLabel: string;
  total?: number;
}) {
  const { t } = useTranslation("dashboard");
  const navigate = useNavigate();
  const more = props.total != null ? props.total - props.rows.length : 0;
  if (props.rows.length === 0) {
    return (
      <p className="rounded-md border border-dashed border-surface-border px-3 py-4 text-center text-xs text-text-muted">
        {props.emptyText}
      </p>
    );
  }
  return (
    <div className="space-y-1.5">
      <ul
        className="divide-y divide-surface-border rounded-md border border-surface-border"
        aria-label={props.ariaLabel}
      >
        {props.rows.map((r) => (
          <li key={r.key}>
            <button
              type="button"
              onClick={() => void navigate(r.to)}
              className="flex min-h-[44px] w-full items-center gap-3 px-3 py-2 text-start text-sm hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate font-medium text-text-primary">{r.primary}</span>
                {r.secondary ? (
                  <span className="block truncate text-xs text-text-muted">{r.secondary}</span>
                ) : null}
              </span>
              {r.badge ? (
                <span
                  className={cn(
                    "shrink-0 rounded-full border px-2 py-0.5 text-[11px] font-medium",
                    mfTone[r.tone ?? "neutral"].chip,
                  )}
                >
                  {r.badge}
                </span>
              ) : null}
              {r.meta ? <span className="shrink-0 text-xs text-text-muted">{r.meta}</span> : null}
              <ChevronRight className="h-3.5 w-3.5 shrink-0 text-text-muted" aria-hidden />
            </button>
          </li>
        ))}
      </ul>
      {more > 0 ? (
        <p className="px-1 text-xs text-text-muted">{t("home.more", { count: more })}</p>
      ) : null}
    </div>
  );
}

/** A plain-language note in place of a figure that cannot be given (never a fake 0). */
export function HomeNote(props: { children: ReactNode; tone?: Tone }) {
  return (
    <p
      className={cn(
        "rounded-md border px-3 py-2 text-xs",
        props.tone ? mfTone[props.tone].chip : "border-surface-border text-text-muted",
      )}
    >
      {props.children}
    </p>
  );
}
