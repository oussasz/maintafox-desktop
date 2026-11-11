/**
 * Business alerts of the bell: how many work orders are late and how many requests are overdue.
 *
 * No new backend rule: these are the figures of the OT and request dashboards, computed by their
 * own commands — `get_wo_stats` (`ot.view`; work still ahead with the planned end passed,
 * `wo::backlog::WO_OVERDUE_SQL`) and `get_di_stats` (`di.view`; open requests past an SLA target).
 * Both are limited in SQL to the sites where the user holds the right (`ScopeSql`), so the bell
 * never counts a record the user cannot open. A background read: failures stay silent.
 */

import { z } from "zod";

import { invokeSilent } from "@/lib/ipc-invoke";

const WoOverdueSchema = z.object({ overdue: z.number() });
const DiOverdueSchema = z.object({ overdue: z.number() });

/** Late work orders in the user's sites. */
export async function getLateWorkOrderCount(): Promise<number> {
  const raw = await invokeSilent<unknown>("get_wo_stats", {});
  return WoOverdueSchema.parse(raw).overdue;
}

/** Overdue requests in the user's sites. */
export async function getOverdueRequestCount(): Promise<number> {
  const raw = await invokeSilent<unknown>("get_di_stats", { filter: {} });
  return DiOverdueSchema.parse(raw).overdue;
}
