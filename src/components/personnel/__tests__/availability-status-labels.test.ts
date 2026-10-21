/**
 * Every availability status the server sends (`personnel::domain::AvailabilityStatus`) has its
 * label in French and English, on the person card and in the availability inspector.
 */
import { describe, expect, it } from "vitest";

import enPersonnel from "@/i18n/locale-data/en/personnel.json";
import frPersonnel from "@/i18n/locale-data/fr/personnel.json";

const AVAILABILITY_STATUSES = [
  "available",
  "assigned",
  "in_training",
  "on_leave",
  "blocked",
  "inactive",
];

type Labels = Record<string, string>;

describe("availability status labels", () => {
  for (const [lang, personnel] of [
    ["en", enPersonnel],
    ["fr", frPersonnel],
  ] as const) {
    it(`are all named in ${lang}`, () => {
      const card = (personnel as unknown as { status: Labels }).status;
      const inspector = (personnel as unknown as { availability: { inspector: { state: Labels } } })
        .availability.inspector.state;
      for (const status of AVAILABILITY_STATUSES) {
        expect(card[status]?.trim(), `status.${status}`).toBeTruthy();
        expect(inspector[status]?.trim(), `availability.inspector.state.${status}`).toBeTruthy();
      }
    });
  }
});
