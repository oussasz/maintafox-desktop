/**
 * How an absence block is named and coloured on the personnel screens (decision register
 * 2026-10-19, Q2 and (b)). The server sends the absence type only with the sensitive-data right
 * (per.sensitiveview) or on the person's own record; otherwise the block is `type_masked` and
 * shown as the neutral tag "Absent" / "Absent — critical for planning", never guessed.
 */
import type { TFunction } from "i18next";

import {
  absenceTimelineState,
  type ResourceTimelineState,
} from "@/design-system/resource-timeline-tokens";
import type { PersonnelAvailabilityBlock } from "@shared/ipc-types";

import type { PersonnelLists } from "./use-personnel-lists";

type Block = Pick<
  PersonnelAvailabilityBlock,
  "block_type" | "type_masked" | "is_critical" | "critical_for_planning"
>;

/** The type's name from the plant's list, or the neutral tag when the type is withheld. */
export function absenceBlockTitle(
  block: Block,
  lists: Pick<PersonnelLists, "text">,
  t: TFunction<"personnel">,
): string {
  if (block.type_masked || block.block_type === undefined) {
    return block.critical_for_planning
      ? t("availability.absenceTag.critical")
      : t("availability.absenceTag.plain");
  }
  return lists.text("absence_types", block.block_type);
}

/** The bar colour: from the type's tag, or critical / plain when the type is withheld. */
export function absenceBlockState(
  block: Block,
  lists: Pick<PersonnelLists, "option">,
): ResourceTimelineState {
  if (block.type_masked || block.block_type === undefined) {
    return block.critical_for_planning ? "critical" : "blocked";
  }
  return absenceTimelineState(
    lists.option("absence_types", block.block_type)?.tag,
    block.is_critical,
  );
}
