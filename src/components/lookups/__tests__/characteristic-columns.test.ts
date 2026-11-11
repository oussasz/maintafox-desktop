import { describe, expect, it } from "vitest";

import {
  EMPTY_CHARACTERISTIC_EDIT,
  buildCharacteristicMetadata,
  characteristicComplete,
  characteristicStateFrom,
  isCharacteristicsDomain,
} from "@/components/lookups/CharacteristicColumns";
import { resolveReferenceValueColumns } from "@/components/lookups/reference-value-columns";
import type { ReferenceDomain, ReferenceValue } from "@shared/ipc-types";

function value(metadata_json: string | null): ReferenceValue {
  return { metadata_json } as unknown as ReferenceValue;
}

describe("technical characteristics in Données de référence", () => {
  it("is the EQUIPMENT.CHARACTERISTICS list, hung under a family", () => {
    expect(isCharacteristicsDomain(" equipment.characteristics ")).toBe(true);
    expect(isCharacteristicsDomain("EQUIPMENT.FAMILY")).toBe(false);
    const flags = resolveReferenceValueColumns({
      id: 1,
      code: "EQUIPMENT.CHARACTERISTICS",
      name: "x",
      structure_type: "flat",
      governance_level: "tenant_managed",
      governance_category: "operational_dictionary",
      audience: "company",
      is_extendable: true,
      validation_rules_json: null,
      created_at: "",
      updated_at: "",
    } as ReferenceDomain);
    expect(flags.showCharacteristic).toBe(true);
    expect(flags.crossDomainParentCode).toBe("EQUIPMENT.FAMILY");
  });

  it("reads and writes type, unit and required, keeping other keys", () => {
    const state = characteristicStateFrom(
      value('{"data_type":"number","unit_code":"KW","required":true,"origin":"shipped"}'),
    );
    expect(state).toEqual({ dataType: "number", unitCode: "KW", required: true });
    const meta = JSON.parse(
      buildCharacteristicMetadata('{"data_type":"number","unit_code":"KW","origin":"shipped"}', {
        dataType: "text",
        unitCode: "KW",
        required: false,
      }),
    ) as Record<string, unknown>;
    // Only a number keeps a unit.
    expect(meta).toEqual({
      data_type: "text",
      unit_code: null,
      required: false,
      origin: "shipped",
    });
  });

  it("needs a value type before saving", () => {
    expect(characteristicComplete(EMPTY_CHARACTERISTIC_EDIT)).toBe(false);
    expect(characteristicComplete({ dataType: "boolean", unitCode: "", required: true })).toBe(
      true,
    );
    expect(characteristicComplete({ dataType: "text", unitCode: "V", required: false })).toBe(
      false,
    );
    expect(characteristicStateFrom(value("not json")).dataType).toBe("");
  });
});
