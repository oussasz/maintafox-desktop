import {
  CalendarClock,
  ChevronRight,
  ClipboardCheck,
  HardHat,
  Info,
  RefreshCw,
  Wrench,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { listWos } from "@/services/wo-service";
import type { WorkOrder } from "@shared/ipc-types";

import { SyncFreshness } from "../components/SyncFreshness";
import { useAfterBackgroundSync, useSyncStatus } from "../hooks/use-sync-status";
import { useMobileT } from "../i18n";
import { formatDateTime, formatHours } from "../lib/format";
import { type UserError, toUserError } from "../lib/user-error";
import { ErrorBanner } from "../ui/feedback";
import {
  BackButton,
  Banner,
  EmptyState,
  NavButton,
  Pill,
  Progress,
  Row,
  Screen,
  Section,
} from "../ui/kit";

/** How many work orders the list asks for in one page. */
const PAGE_SIZE = 50;

type Load =
  | { kind: "loading" }
  | { kind: "error"; error: UserError }
  | { kind: "ok"; items: WorkOrder[]; total: number };

function WorkOrderRow({ wo, onOpen }: { wo: WorkOrder; onOpen: () => void }) {
  const { t, language } = useMobileT();
  const equipment = wo.asset_label ?? wo.asset_code ?? null;
  const planned = formatDateTime(wo.planned_start, language);
  return (
    <button type="button" className="mf-wo" onClick={onOpen}>
      <span className="mf-wo__main">
        <span className="mf-wo__top">
          <span className="mf-wo__code mf-ltr">{wo.code}</span>
          {(wo.status_label ?? wo.status_code) ? (
            <Pill dotColor={wo.status_color}>{wo.status_label ?? wo.status_code}</Pill>
          ) : null}
        </span>
        <span className="mf-wo__title">{wo.title}</span>
        <span className="mf-wo__meta">
          <span>
            <Wrench size={15} aria-hidden />
            {equipment ?? t("wo.noEquipment")}
          </span>
          {planned ? (
            <span>
              <CalendarClock size={15} aria-hidden />
              {planned}
            </span>
          ) : null}
          {wo.urgency_label ? (
            <span>
              <span
                className="mf-pill__dot"
                style={wo.urgency_color ? { background: wo.urgency_color } : undefined}
                aria-hidden
              />
              {wo.urgency_label}
            </span>
          ) : null}
          {wo.requires_permit || wo.requires_shutdown ? (
            <span style={{ color: "var(--mf-orange)" }}>
              <HardHat size={15} aria-hidden />
              {t("wo.safetyShort")}
            </span>
          ) : null}
        </span>
      </span>
      <ChevronRight className="mf-row__chevron mf-mirror" size={20} aria-hidden />
    </button>
  );
}

/** Read-only detail of one work order, with the safety requirements first. */
function WorkOrderDetail({ wo, onBack }: { wo: WorkOrder; onBack: () => void }) {
  const { t, language } = useMobileT();
  const equipment = [wo.asset_code, wo.asset_label].filter(Boolean).join(" — ");
  const planned = formatDateTime(wo.planned_start, language);
  return (
    <Screen
      title={wo.code}
      inlineTitle
      pushed
      withTabs
      leading={<BackButton label={t("wo.back")} onClick={onBack} />}
    >
      <div className="mf-detail-head">
        <h2 className="mf-detail-head__title">{wo.title}</h2>
        <div className="mf-detail-head__pills">
          {(wo.status_label ?? wo.status_code) ? (
            <Pill dotColor={wo.status_color}>{wo.status_label ?? wo.status_code}</Pill>
          ) : null}
          {wo.urgency_label ? <Pill dotColor={wo.urgency_color}>{wo.urgency_label}</Pill> : null}
          {wo.type_label ? <Pill>{wo.type_label}</Pill> : null}
        </div>
      </div>

      {wo.requires_permit || wo.requires_shutdown ? (
        <Banner tone="safety" icon={<HardHat size={24} aria-hidden />} title={t("wo.safetyTitle")}>
          <ul style={{ margin: 0, paddingInlineStart: 18 }}>
            {wo.requires_permit ? (
              <li>
                {wo.permit_rule_permit_type
                  ? t("wo.permitRequiredType", { type: wo.permit_rule_permit_type })
                  : t("wo.permitRequired")}
              </li>
            ) : null}
            {wo.requires_shutdown ? <li>{t("wo.shutdownRequired")}</li> : null}
          </ul>
        </Banner>
      ) : null}

      <Section header={t("wo.details")}>
        <Row title={t("wo.equipment")} detail={equipment || t("wo.noEquipment")} />
        {planned ? <Row title={t("wo.plannedStart")} detail={planned} /> : null}
        {wo.expected_duration_hours != null ? (
          <Row
            title={t("wo.expectedDuration")}
            detail={t("wo.hours", { hours: formatHours(wo.expected_duration_hours, language) })}
          />
        ) : null}
        {wo.shift_class_label ? <Row title={t("wo.shift")} detail={wo.shift_class_label} /> : null}
      </Section>

      {wo.description ? (
        <Section header={t("wo.description")}>
          <div className="mf-row mf-data" style={{ whiteSpace: "pre-wrap" }}>
            {wo.description}
          </div>
        </Section>
      ) : null}

      <Banner tone="info" icon={<Info size={22} aria-hidden />} title={t("wo.executionSoonTitle")}>
        {t("wo.executionSoon")}
      </Banner>
    </Screen>
  );
}

/**
 * "My work orders": the work orders where the signed-in user is the primary responsible and that
 * are not closed yet. The engine filters (`list_wo`, `primary_responsible_id`, `not_closed_only`)
 * and applies the user's site scope; nothing is filtered here.
 */
export function WorkOrdersScreen({ userId, name }: { userId: number; name: string | null }) {
  const { t } = useMobileT();
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [opened, setOpened] = useState<WorkOrder | null>(null);

  const { status } = useSyncStatus();

  // `background`: a sync just finished, so the list is replaced in place without the spinner.
  const run = useCallback(
    async (background = false) => {
      if (!background) setLoad({ kind: "loading" });
      try {
        const page = await listWos({
          primary_responsible_id: userId,
          not_closed_only: true,
          limit: PAGE_SIZE,
          offset: 0,
        });
        setLoad({ kind: "ok", items: page.items, total: page.total });
      } catch (err) {
        setLoad({ kind: "error", error: toUserError(err, "wo.list") });
      }
    },
    [userId],
  );

  useEffect(() => {
    void run();
  }, [run]);

  useAfterBackgroundSync(status, () => void run(true));

  useEffect(() => {
    window.scrollTo(0, 0);
  }, [opened]);

  if (opened) return <WorkOrderDetail wo={opened} onBack={() => setOpened(null)} />;

  const heading =
    load.kind === "ok" && load.total > 0
      ? t("wo.total", { count: load.total })
      : name
        ? t("wo.hello", { name })
        : undefined;
  const subtitle = (
    <>
      {heading}
      <SyncFreshness status={status} />
    </>
  );

  return (
    <Screen
      title={t("wo.title")}
      subtitle={subtitle}
      withTabs
      trailing={
        <NavButton
          label={t("common.refresh")}
          icon={<RefreshCw size={22} aria-hidden />}
          onClick={() => void run()}
          disabled={load.kind === "loading"}
        />
      }
    >
      {load.kind === "loading" ? <Progress text={t("common.loading")} /> : null}
      {load.kind === "error" ? <ErrorBanner error={load.error} onRetry={() => void run()} /> : null}
      {load.kind === "ok" && load.items.length === 0 ? (
        <EmptyState
          icon={<ClipboardCheck size={36} aria-hidden />}
          tone="green"
          title={t("wo.emptyTitle")}
          text={t("wo.empty")}
        />
      ) : null}
      {load.kind === "ok" && load.items.length > 0 ? (
        <Section list label={t("wo.title")}>
          {load.items.map((wo) => (
            <li key={wo.id}>
              <WorkOrderRow wo={wo} onOpen={() => setOpened(wo)} />
            </li>
          ))}
        </Section>
      ) : null}
    </Screen>
  );
}
