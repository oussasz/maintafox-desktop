import { useCallback, useEffect, useState } from "react";

import { listWos } from "@/services/wo-service";
import { toErrorMessage } from "@/utils/errors";
import type { WorkOrder } from "@shared/ipc-types";

import { useMobileT } from "../i18n";

import { ErrorBox } from "./Shell";

/** How many work orders the list asks for in one page. */
const PAGE_SIZE = 50;

type Load =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ok"; items: WorkOrder[]; total: number };

/**
 * "My work orders": the work orders where the signed-in user is the primary responsible and that
 * are not closed yet. The engine filters (`list_wo`, `primary_responsible_id`, `not_closed_only`)
 * and applies the user's site scope; nothing is filtered here. Read only.
 */
export function MyWorkOrders({ userId }: { userId: number }) {
  const { t } = useMobileT();
  const [load, setLoad] = useState<Load>({ kind: "loading" });

  const run = useCallback(async () => {
    setLoad({ kind: "loading" });
    try {
      const page = await listWos({
        primary_responsible_id: userId,
        not_closed_only: true,
        limit: PAGE_SIZE,
        offset: 0,
      });
      setLoad({ kind: "ok", items: page.items, total: page.total });
    } catch (err) {
      setLoad({ kind: "error", message: toErrorMessage(err) });
    }
  }, [userId]);

  useEffect(() => {
    void run();
  }, [run]);

  return (
    <section className="mf-card" aria-labelledby="mf-wo-title">
      <h2 id="mf-wo-title" className="mf-card__title">
        {t("wo.title")}
      </h2>
      {load.kind === "loading" ? (
        <p role="status" className="mf-status">
          {t("common.loading")}
        </p>
      ) : null}
      {load.kind === "error" ? (
        <>
          <ErrorBox title={t("wo.loadError")} message={load.message} />
          <button type="button" className="mf-btn" onClick={() => void run()}>
            {t("common.retry")}
          </button>
        </>
      ) : null}
      {load.kind === "ok" && load.items.length === 0 ? <p>{t("wo.empty")}</p> : null}
      {load.kind === "ok" && load.items.length > 0 ? (
        <>
          <p className="mf-muted">{t("wo.total", { count: load.total })}</p>
          <ul className="mf-list">
            {load.items.map((wo) => (
              <li key={wo.id} className="mf-list__item">
                <div className="mf-list__head">
                  <strong dir="ltr">{wo.code}</strong>
                  <span className="mf-badge">{wo.status_label ?? wo.status_code ?? ""}</span>
                </div>
                <div>{wo.title}</div>
                <div className="mf-muted">
                  {t("wo.equipment")} : {wo.asset_label ?? wo.asset_code ?? t("wo.noEquipment")}
                </div>
              </li>
            ))}
          </ul>
        </>
      ) : null}
    </section>
  );
}
