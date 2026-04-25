/**
 * Home dashboard — submitted DIs awaiting supervisor triage (not yet in the review queue).
 */

import { ChevronDown, ChevronRight, Eye, Inbox } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { listDis } from "@/services/di-service";
import { toErrorMessage } from "@/utils/errors";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

const PREVIEW_LIMIT = 3;

function sortQueue(items: InterventionRequest[]): InterventionRequest[] {
  return [...items].sort((a, b) => a.submitted_at.localeCompare(b.submitted_at));
}

export function DashboardDisTriageInboxPanel() {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const formatShortDate = (iso: string) => {
    try {
      return new Date(iso).toLocaleDateString(dateLocale, {
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
      });
    } catch {
      return iso;
    }
  };
  const navigate = useNavigate();

  const [items, setItems] = useState<InterventionRequest[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setError(null);
    setLoading(true);
    try {
      const page = await listDis({
        status: ["submitted"],
        limit: 200,
        offset: 0,
      });
      setItems(page.items);
    } catch (e) {
      setError(toErrorMessage(e));
      setItems([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    const onRefresh = () => {
      void load();
    };
    window.addEventListener("mf:di-triage-refresh", onRefresh);
    return () => window.removeEventListener("mf:di-triage-refresh", onRefresh);
  }, [load]);

  const sorted = useMemo(() => sortQueue(items), [items]);
  const previewItems = useMemo(() => sorted.slice(0, PREVIEW_LIMIT), [sorted]);
  const hiddenInPreview = Math.max(0, sorted.length - previewItems.length);

  if (!loading && sorted.length === 0) {
    return null;
  }

  return (
    <Card className="border-slate-200 bg-slate-50/40">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-4 py-3 text-left text-sm font-medium"
        onClick={() => setCollapsed((c) => !c)}
      >
        {collapsed ? (
          <ChevronRight className="h-4 w-4 shrink-0" />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0" />
        )}
        <Inbox className="h-4 w-4 shrink-0 text-slate-600" />
        <span>{t("triage.panelTitle")}</span>
        <Badge variant="secondary" className="text-[10px] h-5 min-w-[20px] justify-center">
          {loading ? "…" : sorted.length}
        </Badge>
      </button>

      {!collapsed && (
        <CardContent className="p-0">
          {error && (
            <div
              role="alert"
              className="mx-4 mt-3 rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              {error}
            </div>
          )}
          {loading ? (
            <p className="px-4 py-3 text-xs text-text-muted">{t("triage.loading")}</p>
          ) : (
            <div className="divide-y divide-slate-200/80">
              {previewItems.map((di) => (
                <div
                  key={di.id}
                  className="flex items-center gap-3 px-4 py-2.5 text-xs hover:bg-muted/50"
                >
                  <span className="font-mono text-muted-foreground shrink-0 w-[80px]">
                    {di.code}
                  </span>
                  <span className="truncate min-w-0 flex-1 font-medium">{di.title}</span>
                  <span className="text-muted-foreground shrink-0 w-[60px] text-right">
                    #{di.submitter_id}
                  </span>
                  <span className="text-muted-foreground shrink-0 w-[80px] text-right">
                    {formatShortDate(di.submitted_at)}
                  </span>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 w-6 p-0 shrink-0"
                    onClick={() => navigate(`/requests?openDi=${di.id}`)}
                    title={t("review.view")}
                  >
                    <Eye className="h-3.5 w-3.5" />
                  </Button>
                </div>
              ))}
            </div>
          )}
          {hiddenInPreview > 0 && !loading && (
            <div className="border-t border-slate-200/80 bg-slate-50/50 px-4 py-2.5 text-center">
              <Link
                to="/requests?triage=1"
                className="inline-flex items-center justify-center gap-1.5 text-xs font-medium text-slate-800 hover:text-slate-950 hover:underline"
              >
                {t("triage.seeAllInList", { more: hiddenInPreview })}
                <ChevronRight className="h-3.5 w-3.5 shrink-0 opacity-70" />
              </Link>
            </div>
          )}
        </CardContent>
      )}
    </Card>
  );
}
