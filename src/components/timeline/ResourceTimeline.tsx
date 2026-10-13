/**
 * ResourceTimeline — Design System resource × time primitive.
 * Wraps react-calendar-timeline. Feature modules must not import the library directly.
 * Visual language: resource-timeline.css + theme tokens (library CSS is structural only).
 */

import { Maximize2, ZoomIn, ZoomOut } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import Timeline, {
  DateHeader,
  SidebarHeader,
  TimelineHeaders,
  type TimelineGroupBase,
  type TimelineItemBase,
} from "react-calendar-timeline";
import { useTranslation } from "react-i18next";
import "react-calendar-timeline/style.css";

import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import {
  RESOURCE_TIMELINE_ITEM_HEIGHT_RATIO,
  RESOURCE_TIMELINE_LINE_HEIGHT,
  RESOURCE_TIMELINE_MIN_HEIGHT,
  RESOURCE_TIMELINE_SIDEBAR_WIDTH,
  resourceTimelineItemStateClass,
  type ResourceTimelineState,
} from "@/design-system/resource-timeline-tokens";
import "@/design-system/resource-timeline.css";
import { cn } from "@/lib/utils";

const MS_HOUR = 60 * 60 * 1000;
const MS_DAY = 24 * MS_HOUR;

const DEFAULT_KEYS = {
  groupIdKey: "id",
  groupTitleKey: "title",
  groupLabelKey: "title",
  groupRightTitleKey: "rightTitle",
  itemIdKey: "id",
  itemTitleKey: "title",
  itemDivTitleKey: "title",
  itemGroupKey: "group",
  itemTimeStartKey: "start_time",
  itemTimeEndKey: "end_time",
} as const;

const DEFAULT_TIME_STEPS = {
  second: 1,
  minute: 1,
  hour: 1,
  day: 1,
  month: 1,
  year: 1,
};

export type ResourceTimelineGroup = TimelineGroupBase & {
  id: string | number;
  title: ReactNode;
};

export type ResourceTimelineItem = TimelineItemBase<number> & {
  id: string | number;
  group: string | number;
  title?: ReactNode;
  start_time: number;
  end_time: number;
  /** Semantic color state — resolved via DS token classes */
  state?: ResourceTimelineState;
  canMove?: boolean;
  canResize?: boolean;
};

export type ResourceTimelineTimeRangeHighlight = {
  start: number;
  end: number;
  state: ResourceTimelineState;
};

export type ResourceTimelineProps = {
  groups: ResourceTimelineGroup[];
  items: ResourceTimelineItem[];
  visibleTimeStart: number;
  visibleTimeEnd: number;
  /**
   * The intended time range for the "fit" button and zoom clamping.
   * Defaults to the initial visibleTimeStart/End if not provided.
   * Pass the locked date-range bounds (not updated on pan) so fit always
   * resets to the full intended horizon.
   */
  horizonStart?: number;
  horizonEnd?: number;
  onVisibleTimeChange?: (start: number, end: number) => void;
  onItemSelect?: (itemId: string | number) => void;
  onGroupClick?: (groupId: string | number) => void;
  /**
   * When provided, items with `canMove: true` become draggable.
   * Availability and other read-only adapters omit this prop.
   */
  onItemMove?: (itemId: string | number, newStartMs: number, newGroupId: string | number) => void;
  /** Shaded vertical ranges (freeze / lock windows) — token state classes only. */
  timeRangeHighlights?: ResourceTimelineTimeRangeHighlight[];
  selectedItemIds?: Array<string | number> | undefined;
  loading?: boolean;
  error?: string | null;
  emptyTitle?: string;
  className?: string;
  sidebarWidth?: number;
  lineHeight?: number;
  /** Fixed height in px. Omit to fill parent via ResizeObserver. */
  height?: number;
  showZoomControls?: boolean;
  /**
   * Max visible span in milliseconds. Defaults to 90 days (Availability).
   * Planning may pass a wider span so long DU/AU horizons stay in sync.
   */
  maxZoom?: number;
};

function isSelectedId(id: string | number, selected: Array<string | number> | undefined): boolean {
  if (!selected || selected.length === 0) return false;
  return selected.some((s) => String(s) === String(id));
}

export function ResourceTimeline({
  groups,
  items,
  visibleTimeStart,
  visibleTimeEnd,
  horizonStart: horizonStartProp,
  horizonEnd: horizonEndProp,
  onVisibleTimeChange,
  onItemSelect,
  onGroupClick,
  onItemMove,
  timeRangeHighlights,
  selectedItemIds,
  loading = false,
  error = null,
  emptyTitle,
  className,
  sidebarWidth = RESOURCE_TIMELINE_SIDEBAR_WIDTH,
  lineHeight = RESOURCE_TIMELINE_LINE_HEIGHT,
  height,
  showZoomControls = true,
  maxZoom = MS_DAY * 90,
}: ResourceTimelineProps) {
  const { t } = useTranslation("common");
  const [internalStart, setInternalStart] = useState(visibleTimeStart);
  const [internalEnd, setInternalEnd] = useState(visibleTimeEnd);
  const measureRef = useRef<HTMLDivElement>(null);
  const [measuredHeight, setMeasuredHeight] = useState(RESOURCE_TIMELINE_MIN_HEIGHT);
  const moveEnabled = typeof onItemMove === "function";

  useEffect(() => {
    if (!onVisibleTimeChange) {
      setInternalStart(visibleTimeStart);
      setInternalEnd(visibleTimeEnd);
    }
  }, [visibleTimeStart, visibleTimeEnd, onVisibleTimeChange]);

  useEffect(() => {
    if (height != null) return;
    const el = measureRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;

    const apply = (next: number) => {
      const clamped = Math.max(RESOURCE_TIMELINE_MIN_HEIGHT, Math.floor(next));
      setMeasuredHeight((prev) => (prev === clamped ? prev : clamped));
    };

    apply(el.clientHeight);
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      apply(entry.contentRect.height);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [height]);

  const start = onVisibleTimeChange ? visibleTimeStart : internalStart;
  const end = onVisibleTimeChange ? visibleTimeEnd : internalEnd;

  // horizonStart/End are the *intended* date-range bounds used for fit and
  // zoom clamping. They must not change when the user pans, which is why
  // the caller should pass them separately from visibleTimeStart/End.
  // Fallback: capture the initial values via a ref so an uncontrolled
  // caller still gets a meaningful fit target.
  const initialHorizonRef = useRef({ start: visibleTimeStart, end: visibleTimeEnd });
  const horizonStart = horizonStartProp ?? initialHorizonRef.current.start;
  const horizonEnd = horizonEndProp ?? initialHorizonRef.current.end;

  const canvasHeight = height ?? measuredHeight;

  const coloredItems = useMemo(() => {
    return items.map((item) => {
      const stateClass = item.state
        ? resourceTimelineItemStateClass(item.state)
        : "mf-rt-item mf-rt-item--unset";
      const selected = isSelectedId(item.id, selectedItemIds);
      const itemCanMove = moveEnabled && item.canMove === true;
      return {
        ...item,
        canMove: itemCanMove,
        canResize: false,
        canChangeGroup: itemCanMove,
        className: cn(stateClass, selected && "mf-rt-item--selected", item.className),
      };
    });
  }, [items, selectedItemIds, moveEnabled]);

  const verticalLineClassNamesForTime = useCallback(
    (timeStart: number, timeEnd: number) => {
      if (!timeRangeHighlights || timeRangeHighlights.length === 0) return [];
      const classes: string[] = [];
      for (const highlight of timeRangeHighlights) {
        if (timeStart < highlight.end && timeEnd > highlight.start) {
          classes.push(`mf-rt-col mf-rt-col--${highlight.state}`);
        }
      }
      return classes;
    },
    [timeRangeHighlights],
  );

  const handleItemMove = useCallback(
    (itemId: string | number, dragTime: number, newGroupOrder: number) => {
      if (!onItemMove) return;
      const group = groups[newGroupOrder];
      if (!group) return;
      onItemMove(itemId, dragTime, group.id);
    },
    [onItemMove, groups],
  );

  const handleTimeChange = useCallback(
    (
      visibleTimeStartNext: number,
      visibleTimeEndNext: number,
      updateScrollCanvas: (s: number, e: number) => void,
    ) => {
      updateScrollCanvas(visibleTimeStartNext, visibleTimeEndNext);
      if (onVisibleTimeChange) {
        onVisibleTimeChange(visibleTimeStartNext, visibleTimeEndNext);
      } else {
        setInternalStart(visibleTimeStartNext);
        setInternalEnd(visibleTimeEndNext);
      }
    },
    [onVisibleTimeChange],
  );

  const zoomBy = useCallback(
    (factor: number) => {
      const mid = (start + end) / 2;
      const half = ((end - start) / 2) * factor;
      const nextStart = Math.max(horizonStart - MS_DAY, mid - half);
      const nextEnd = Math.min(horizonEnd + MS_DAY, mid + half);
      if (nextEnd - nextStart < MS_HOUR) return;
      if (onVisibleTimeChange) {
        onVisibleTimeChange(nextStart, nextEnd);
      } else {
        setInternalStart(nextStart);
        setInternalEnd(nextEnd);
      }
    },
    [start, end, horizonStart, horizonEnd, onVisibleTimeChange],
  );

  const fitHorizon = useCallback(() => {
    if (onVisibleTimeChange) {
      onVisibleTimeChange(horizonStart, horizonEnd);
    } else {
      setInternalStart(horizonStart);
      setInternalEnd(horizonEnd);
    }
  }, [horizonStart, horizonEnd, onVisibleTimeChange]);

  const zoomToolbar =
    showZoomControls && !error && !loading && groups.length > 0 ? (
      <div className="mf-rt-toolbar">
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2"
          onClick={() => zoomBy(0.7)}
          title={t("resourceTimeline.zoomIn")}
          aria-label={t("resourceTimeline.zoomIn")}
        >
          <ZoomIn className="h-3.5 w-3.5" />
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2"
          onClick={() => zoomBy(1.4)}
          title={t("resourceTimeline.zoomOut")}
          aria-label={t("resourceTimeline.zoomOut")}
        >
          <ZoomOut className="h-3.5 w-3.5" />
        </Button>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 px-2"
          onClick={fitHorizon}
          title={t("resourceTimeline.fitHorizon")}
          aria-label={t("resourceTimeline.fitHorizon")}
        >
          <Maximize2 className="h-3.5 w-3.5" />
        </Button>
      </div>
    ) : null;

  let canvasBody: ReactNode;
  if (error) {
    canvasBody = <div className="mf-rt-status mf-rt-status--error">{error}</div>;
  } else if (loading) {
    canvasBody = (
      <div className="mf-rt-status mf-rt-status--loading">{t("resourceTimeline.loading")}</div>
    );
  } else if (groups.length === 0) {
    canvasBody = (
      <div className="mf-rt-status">
        <EmptyState title={emptyTitle ?? t("resourceTimeline.empty")} />
      </div>
    );
  } else {
    canvasBody = (
      <div style={{ height: canvasHeight }} className="h-full min-h-0 w-full">
        <Timeline
          groups={groups}
          items={coloredItems}
          keys={DEFAULT_KEYS}
          defaultTimeStart={start}
          defaultTimeEnd={end}
          visibleTimeStart={start}
          visibleTimeEnd={end}
          sidebarWidth={sidebarWidth}
          rightSidebarWidth={0}
          lineHeight={lineHeight}
          itemHeightRatio={RESOURCE_TIMELINE_ITEM_HEIGHT_RATIO}
          minZoom={MS_HOUR * 6}
          maxZoom={maxZoom}
          canMove={moveEnabled}
          canResize={false}
          canChangeGroup={moveEnabled}
          canSelect
          stackItems={false}
          timeSteps={DEFAULT_TIME_STEPS}
          selected={selectedItemIds}
          onTimeChange={handleTimeChange}
          onItemSelect={(itemId) => onItemSelect?.(itemId)}
          onItemClick={(itemId) => onItemSelect?.(itemId)}
          onItemMove={moveEnabled ? handleItemMove : undefined}
          onCanvasClick={(groupId) => onGroupClick?.(groupId)}
          verticalLineClassNamesForTime={
            timeRangeHighlights && timeRangeHighlights.length > 0
              ? verticalLineClassNamesForTime
              : undefined
          }
          buffer={1}
        >
          <TimelineHeaders>
            <SidebarHeader>
              {({ getRootProps }) => {
                // Destructure style so we can override color explicitly.
                // The library injects width/height via style; it also
                // inherits a light/white color from rct-header-root in
                // some themes. Overriding via CSS selector is fragile
                // because specificity and inline-style order vary across
                // library versions. Using an explicit style here is safe.
                const { style: libStyle, ...rootProps } = getRootProps();
                return (
                  <div
                    {...rootProps}
                    className={cn(
                      rootProps.className,
                      "mf-rt-sidebar-header flex h-full items-center px-2 text-xs font-medium",
                    )}
                    style={{
                      ...libStyle,
                      background: "var(--rt-header-bg)",
                      color: "var(--text-secondary)",
                    }}
                  >
                    {t("resourceTimeline.sidebarLabel")}
                  </div>
                );
              }}
            </SidebarHeader>
            <DateHeader unit="primaryHeader" />
            <DateHeader />
          </TimelineHeaders>
        </Timeline>
      </div>
    );
  }

  return (
    <div className={cn("mf-resource-timeline flex min-h-0 flex-1 flex-col", className)}>
      <div className="mf-rt-frame">
        {zoomToolbar}
        <div ref={measureRef} className="mf-rt-canvas">
          {canvasBody}
        </div>
      </div>
    </div>
  );
}
