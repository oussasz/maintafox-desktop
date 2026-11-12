import { describe, expect, it } from "vitest";

import {
  characteristicErrors,
  isNumberText,
} from "@/components/assets/detail/AssetCharacteristicsCard";
import type { AssetCharacteristic } from "@shared/ipc-types";

function item(partial: Partial<AssetCharacteristic> & { code: string }): AssetCharacteristic {
  return {
    label: partial.code,
    data_type: "text",
    unit_code: null,
    unit_symbol: null,
    required: false,
    definition_problem: null,
    value: null,
    ...partial,
  };
}

describe("asset technical characteristics form", () => {
  it("accepts numbers with a decimal comma", () => {
    expect(isNumberText("7,5")).toBe(true);
    expect(isNumberText(" 1 200 ")).toBe(true);
    expect(isNumberText("sept")).toBe(false);
    expect(isNumberText("")).toBe(false);
  });

  it("flags required fields left empty and numbers that are not numbers", () => {
    const items = [
      item({ code: "POWER", data_type: "number", required: true }),
      item({ code: "VOLTAGE", data_type: "number" }),
      item({ code: "RANGE", data_type: "text", required: true }),
    ];
    expect(characteristicErrors(items, { POWER: "", VOLTAGE: "abc", RANGE: "0-10 bar" })).toEqual({
      POWER: "required",
      VOLTAGE: "number",
    });
    expect(characteristicErrors(items, { POWER: "15,5", VOLTAGE: "", RANGE: "x" })).toEqual({});
  });
});
