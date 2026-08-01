/**
 * OrgDesignerPropertyPanel — enterprise-style right property chrome.
 *
 * Desktop: horizontally resizable (left-edge drag), collapsible, fixed bounds.
 * Narrow viewports: tree stays full-width; inspector/audit open in a Sheet drawer.
 * Does not own node business logic — hosts NodeInspectorPanel + AuditTimeline.
 */

import { PanelRightClose, PanelRightOpen } from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useTranslation } from "react-i18next";

import { AuditTimeline } from "@/components/org/AuditTimeline";
import { NodeInspectorPanel } from "@/components/org/NodeInspectorPanel";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useMediaQuery } from "@/hooks/useMediaQuery";
import { cn } from "@/lib/utils";
import { useOrgDesignerStore } from "@/stores/org-designer-store";

const DEFAULT_WIDTH = 360;
const MIN_WIDTH = 320;
const MAX_WIDTH = 600;
const WIDTH_STORAGE_KEY = "org-designer-inspector-width";
const COLLAPSED_STORAGE_KEY = "org-designer-inspector-collapsed";
/** Match Tailwind `lg` — below this, use drawer instead of docked panel. */
const DESKTOP_QUERY = "(min-width: 1024px)";

function clampWidth(width: number): number {
  return Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, Math.round(width)));
}

function readStoredWidth(): number {
  try {
    const raw = localStorage.getItem(WIDTH_STORAGE_KEY);
    if (!raw) return DEFAULT_WIDTH;
    const n = Number(raw);
    return Number.isFinite(n) ? clampWidth(n) : DEFAULT_WIDTH;
  } catch {
    return DEFAULT_WIDTH;
  }
}

function readStoredCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

function PropertyPanelTabs({ readOnly }: { readOnly: boolean }) {
  const { t } = useTranslation("org");

  return (
    <Tabs defaultValue="inspector" className="flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <div className="shrink-0 overflow-x-auto overscroll-x-contain border-b border-surface-border">
        <TabsList className="mx-2 mt-2 mb-0 inline-flex h-9 w-max min-w-0 justify-start gap-0.5 rounded-md bg-muted p-1">
          <TabsTrigger value="inspector" className="shrink-0 px-3">
            {t("designer.inspectorTab")}
          </TabsTrigger>
          <TabsTrigger value="audit" className="shrink-0 px-3">
            {t("designer.auditTab")}
          </TabsTrigger>
        </TabsList>
      </div>
      <TabsContent
        value="inspector"
        className="mt-0 flex min-h-0 flex-1 flex-col overflow-hidden p-0 data-[state=inactive]:hidden"
      >
        <NodeInspectorPanel readOnly={readOnly} />
      </TabsContent>
      <TabsContent
        value="audit"
        className="mt-0 min-h-0 flex-1 overflow-y-auto p-0 data-[state=inactive]:hidden"
      >
        <AuditTimeline />
      </TabsContent>
    </Tabs>
  );
}

interface OrgDesignerPropertyPanelProps {
  readOnly: boolean;
}

export function OrgDesignerPropertyPanel({ readOnly }: OrgDesignerPropertyPanelProps) {
  const { t } = useTranslation("org");
  const isDesktop = useMediaQuery(DESKTOP_QUERY, true);
  const selectedNodeId = useOrgDesignerStore((s) => s.selectedNodeId);

  const [width, setWidth] = useState(readStoredWidth);
  const [collapsed, setCollapsed] = useState(readStoredCollapsed);
  const [drawerOpen, setDrawerOpen] = useState(false);

  const dragRef = useRef<{ startX: number; startWidth: number } | null>(null);

  useEffect(() => {
    try {
      localStorage.setItem(WIDTH_STORAGE_KEY, String(width));
    } catch {
      /* ignore quota */
    }
  }, [width]);

  useEffect(() => {
    try {
      localStorage.setItem(COLLAPSED_STORAGE_KEY, collapsed ? "1" : "0");
    } catch {
      /* ignore quota */
    }
  }, [collapsed]);

  // On narrow viewports, open the drawer when a node is selected.
  useEffect(() => {
    if (!isDesktop && selectedNodeId != null) {
      setDrawerOpen(true);
    }
  }, [isDesktop, selectedNodeId]);

  const persistCollapse = useCallback((next: boolean) => {
    setCollapsed(next);
  }, []);

  const onResizePointerDown = useCallback(
    (e: ReactPointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      e.currentTarget.setPointerCapture(e.pointerId);
      dragRef.current = { startX: e.clientX, startWidth: width };
    },
    [width],
  );

  const onResizePointerMove = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag) return;
    // Dragging the left edge: move left → wider panel.
    const delta = drag.startX - e.clientX;
    setWidth(clampWidth(drag.startWidth + delta));
  }, []);

  const onResizePointerUp = useCallback((e: ReactPointerEvent<HTMLDivElement>) => {
    if (dragRef.current) {
      dragRef.current = null;
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {
        /* already released */
      }
    }
  }, []);

  if (!isDesktop) {
    return (
      <>
        <div className="flex w-10 shrink-0 flex-col items-center border-l border-surface-border bg-surface-1 py-2">
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="h-8 w-8"
            onClick={() => setDrawerOpen(true)}
            title={t("designer.propertyPanel.open")}
            aria-label={t("designer.propertyPanel.open")}
          >
            <PanelRightOpen className="h-4 w-4" />
          </Button>
        </div>

        <Sheet open={drawerOpen} onOpenChange={setDrawerOpen}>
          <SheetContent
            side="right"
            className="flex w-full flex-col gap-0 p-0 sm:max-w-[min(100vw,600px)]"
          >
            <SheetHeader className="space-y-1 border-b border-surface-border px-4 py-3 text-left">
              <SheetTitle className="text-sm font-semibold">
                {t("designer.propertyPanel.title")}
              </SheetTitle>
              <SheetDescription className="text-xs text-text-muted">
                {t("designer.propertyPanel.drawerDescription")}
              </SheetDescription>
            </SheetHeader>
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              <PropertyPanelTabs readOnly={readOnly} />
            </div>
          </SheetContent>
        </Sheet>
      </>
    );
  }

  if (collapsed) {
    return (
      <div className="flex w-10 shrink-0 flex-col items-center border-l border-surface-border bg-surface-1 py-2">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-8 w-8"
          onClick={() => persistCollapse(false)}
          title={t("designer.propertyPanel.expand")}
          aria-label={t("designer.propertyPanel.expand")}
        >
          <PanelRightOpen className="h-4 w-4" />
        </Button>
      </div>
    );
  }

  return (
    <aside
      className="relative flex min-h-0 shrink-0 flex-col self-stretch overflow-hidden border-l border-surface-border bg-surface-0"
      style={{ width }}
      aria-label={t("designer.propertyPanel.title")}
    >
      {/* Left-edge resize handle */}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        aria-valuenow={width}
        aria-label={t("designer.propertyPanel.resize")}
        tabIndex={0}
        className={cn(
          "absolute inset-y-0 left-0 z-10 w-1.5 cursor-col-resize touch-none",
          "hover:bg-primary/20 active:bg-primary/30",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
        )}
        onPointerDown={onResizePointerDown}
        onPointerMove={onResizePointerMove}
        onPointerUp={onResizePointerUp}
        onPointerCancel={onResizePointerUp}
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") {
            e.preventDefault();
            setWidth((w) => clampWidth(w + 16));
          } else if (e.key === "ArrowRight") {
            e.preventDefault();
            setWidth((w) => clampWidth(w - 16));
          } else if (e.key === "Home") {
            e.preventDefault();
            setWidth(MIN_WIDTH);
          } else if (e.key === "End") {
            e.preventDefault();
            setWidth(MAX_WIDTH);
          }
        }}
      />

      <div className="flex shrink-0 items-center justify-end gap-1 border-b border-surface-border/60 px-1 py-0.5">
        <Button
          type="button"
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => persistCollapse(true)}
          title={t("designer.propertyPanel.collapse")}
          aria-label={t("designer.propertyPanel.collapse")}
        >
          <PanelRightClose className="h-3.5 w-3.5" />
        </Button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <PropertyPanelTabs readOnly={readOnly} />
      </div>
    </aside>
  );
}
