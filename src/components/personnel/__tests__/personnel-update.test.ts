import { describe, expect, it } from "vitest";

import { clearableChanges } from "@/components/personnel/personnel-update";

const BEFORE = {
  supervisor_id: 12,
  hire_date: "2024-03-01",
  termination_date: null,
  email: "amina@plant.dz",
  phone: "0550 00 00 00",
  external_company_id: null,
  contract_number: null,
  contract_start_date: null,
  contract_end_date: null,
  notes: "Night shift",
};

describe("person update: optional fields", () => {
  it("sends an emptied field as null so it is cleared", () => {
    expect(clearableChanges(BEFORE, { ...BEFORE, phone: null, supervisor_id: null })).toEqual({
      phone: null,
      supervisor_id: null,
    });
  });

  it("leaves untouched fields out", () => {
    expect(clearableChanges(BEFORE, { ...BEFORE })).toEqual({});
  });

  it("sends a changed value", () => {
    expect(clearableChanges(BEFORE, { ...BEFORE, email: "a.b@plant.dz", notes: null })).toEqual({
      email: "a.b@plant.dz",
      notes: null,
    });
  });

  it("does not send a field that was already empty and stays empty", () => {
    expect(clearableChanges(BEFORE, { ...BEFORE, contract_number: null })).toEqual({});
  });
});
