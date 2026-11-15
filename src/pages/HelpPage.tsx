import { BookOpen, HelpCircle, Search } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { canSeeNavItem } from "@/components/layout/Sidebar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useModuleCapabilities } from "@/hooks/use-module-capabilities";
import { usePermissions } from "@/hooks/use-permissions";
import { isModuleCapabilityAllowed } from "@/lib/module-capability";
import { defaultNavItems } from "@/navigation/nav-registry";

import { HELP_GUIDES, type HelpGuide } from "./help/help-guides";

function stepKeys(guide: HelpGuide): string[] {
  return Array.from({ length: guide.steps }, (_, i) => `modules.${guide.id}.step${i + 1}`);
}

/**
 * In-app user guide, open to every signed-in user. A module's guide is listed (and linked) only
 * when the sidebar would show that module to this user: same right, same licence check.
 */
export function HelpPage() {
  const { t } = useTranslation("help");
  const { can } = usePermissions();
  const { capabilityMap } = useModuleCapabilities();
  const [query, setQuery] = useState("");

  const visibleGuides = useMemo(
    () =>
      HELP_GUIDES.filter((guide) => {
        if (guide.requiredPermission && !can(guide.requiredPermission)) return false;
        const item = guide.navKey ? defaultNavItems.find((i) => i.key === guide.navKey) : undefined;
        if (guide.navKey && !item) return false;
        if (!item) return true;
        return (
          canSeeNavItem(item, can) &&
          isModuleCapabilityAllowed(capabilityMap, item.requiredPermission)
        );
      }),
    [can, capabilityMap],
  );

  const needle = query.trim().toLowerCase();
  const guides = needle
    ? visibleGuides.filter((guide) =>
        [
          t(`modules.${guide.id}.title` as never),
          t(`modules.${guide.id}.purpose` as never),
          ...stepKeys(guide).map((k) => t(k as never)),
        ]
          .join(" ")
          .toLowerCase()
          .includes(needle),
      )
    : visibleGuides;

  return (
    <ModulePageShell
      icon={HelpCircle}
      title={t("page.title")}
      description={t("page.subtitle")}
      filterBar={
        <div className="relative w-full max-w-md">
          <Search
            className="pointer-events-none absolute start-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-text-muted"
            aria-hidden
          />
          <Input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label={t("page.searchLabel")}
            placeholder={t("page.searchPlaceholder")}
            className="ps-8"
          />
        </div>
      }
    >
      <div className="mx-auto flex max-w-5xl flex-col gap-6">
        <section
          aria-labelledby="help-intro"
          className="rounded-lg border border-surface-border bg-surface-1 p-4"
        >
          <h2 id="help-intro" className="text-base font-semibold text-text-primary">
            {t("intro.title")}
          </h2>
          <p className="mt-1 text-sm text-text-secondary">{t("intro.body")}</p>
          <ol className="mt-3 list-decimal space-y-1 ps-5 text-sm text-text-secondary">
            {[1, 2, 3, 4].map((n) => (
              <li key={n}>{t(`intro.step${n}` as never)}</li>
            ))}
          </ol>
        </section>

        <section aria-labelledby="help-modules" className="flex flex-col gap-3">
          <div>
            <h2 id="help-modules" className="text-base font-semibold text-text-primary">
              {t("page.modulesTitle")}
            </h2>
            <p className="text-sm text-text-muted">{t("page.modulesHint")}</p>
          </div>
          {guides.length === 0 ? (
            <p role="status" className="text-sm text-text-muted">
              {t("page.noResults")}
            </p>
          ) : (
            <div className="grid gap-3 md:grid-cols-2">
              {guides.map((guide) => {
                const item = guide.navKey
                  ? defaultNavItems.find((i) => i.key === guide.navKey)
                  : undefined;
                const path = guide.path ?? item?.path;
                return (
                  <article
                    key={guide.id}
                    data-testid={`help-guide-${guide.id}`}
                    className="flex flex-col gap-2 rounded-lg border border-surface-border bg-surface-1 p-4"
                  >
                    <h3 className="flex items-center gap-2 text-sm font-semibold text-text-primary">
                      <BookOpen className="h-4 w-4 text-primary" aria-hidden />
                      {t(`modules.${guide.id}.title` as never)}
                    </h3>
                    <p className="text-sm text-text-secondary">
                      <span className="font-medium">{t("page.purposeLabel")} </span>
                      {t(`modules.${guide.id}.purpose` as never)}
                    </p>
                    <div>
                      <p className="text-xs font-semibold uppercase tracking-wider text-text-muted">
                        {t("page.stepsLabel")}
                      </p>
                      <ol className="mt-1 list-decimal space-y-0.5 ps-5 text-sm text-text-secondary">
                        {stepKeys(guide).map((key) => (
                          <li key={key}>{t(key as never)}</li>
                        ))}
                      </ol>
                    </div>
                    {path ? (
                      <Button asChild variant="outline" size="sm" className="mt-auto self-start">
                        <Link to={path}>{t("page.open")}</Link>
                      </Button>
                    ) : null}
                  </article>
                );
              })}
            </div>
          )}
        </section>

        <section
          aria-labelledby="help-good"
          className="rounded-lg border border-surface-border bg-surface-1 p-4"
        >
          <h2 id="help-good" className="text-base font-semibold text-text-primary">
            {t("good.title")}
          </h2>
          <dl className="mt-2 space-y-3 text-sm">
            {(
              [
                ["rolesTitle", "roles"],
                ["syncTitle", "sync"],
                ["docsTitle", "docs"],
              ] as const
            ).map(([titleKey, bodyKey]) => (
              <div key={bodyKey}>
                <dt className="font-medium text-text-primary">{t(`good.${titleKey}` as never)}</dt>
                <dd className="text-text-secondary">{t(`good.${bodyKey}` as never)}</dd>
              </div>
            ))}
          </dl>
        </section>
      </div>
    </ModulePageShell>
  );
}
