/**
 * Permit requirement rules in Données de référence: a rule is written in the value's metadata with
 * its class / work type and permit type, other keys kept; the editor picks each from its list and
 * marks a rule that does not name what it needs.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ReferenceValue } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
const ref = vi.hoisted(() => ({ listPublishedReferenceValuesByDomainCode: vi.fn() }));
vi.mock("@/services/reference-service", () => ref);
const permits = vi.hoisted(() => ({ listPermitTypes: vi.fn() }));
vi.mock("@/services/permit-service", () => permits);

import {
  EMPTY_PERMIT_RULE_EDIT,
  PermitRuleEditCells,
  PermitRuleReadCells,
  type PermitRuleOptions,
  buildPermitRuleMetadata,
  isPermitRulesDomain,
  permitRuleComplete,
  permitRuleProblem,
  permitRuleStateFrom,
  unusablePermitRules,
  usePermitRuleOptions,
} from "../PermitRuleColumns";

function value(metadata: string | null): ReferenceValue {
  return { id: 1, code: "R1", label: "Pompes", metadata_json: metadata } as ReferenceValue;
}

const options: PermitRuleOptions = {
  classes: [{ code: "PUMP", label: "Pompes" }],
  workTypes: [{ code: "CORR", label: "Correctif" }],
  permitTypes: [{ code: "HOT_WORK", label: "Travail à chaud" }],
  canReadPermitTypes: true,
  loaded: true,
  loading: false,
  error: null,
  reload: vi.fn(),
};

beforeEach(() => vi.clearAllMocks());

describe("permit rule metadata", () => {
  it("reads and writes the rule, keeping other keys", () => {
    const v = value('{"equipment_class_code":"PUMP","permit_type_code":"HOT_WORK","origin":"x"}');
    const state = permitRuleStateFrom(v);
    expect(state).toEqual({ classCode: "PUMP", workTypeCode: "", permitTypeCode: "HOT_WORK" });
    const written = JSON.parse(
      buildPermitRuleMetadata(v.metadata_json, { ...state, classCode: "", workTypeCode: "CORR" }),
    ) as Record<string, unknown>;
    expect(written).toEqual({ origin: "x", work_type_code: "CORR", permit_type_code: "HOT_WORK" });
  });

  it("needs a permit type and a class or a work type", () => {
    expect(permitRuleComplete(EMPTY_PERMIT_RULE_EDIT)).toBe(false);
    expect(permitRuleComplete({ classCode: "PUMP", workTypeCode: "", permitTypeCode: "" })).toBe(
      false,
    );
    expect(
      permitRuleComplete({ classCode: "", workTypeCode: "", permitTypeCode: "HOT_WORK" }),
    ).toBe(false);
    expect(
      permitRuleComplete({ classCode: "", workTypeCode: "CORR", permitTypeCode: "HOT_WORK" }),
    ).toBe(true);
    expect(isPermitRulesDomain("permit.requirement_rules")).toBe(true);
    expect(isPermitRulesDomain("EQUIPMENT.CLASS")).toBe(false);
  });
});

describe("PermitRuleEditCells", () => {
  it("picks each field from its list and says what is missing", () => {
    const onChange = vi.fn();
    render(
      <table>
        <tbody>
          <tr>
            <PermitRuleEditCells
              state={EMPTY_PERMIT_RULE_EDIT}
              onChange={onChange}
              options={options}
            />
          </tr>
        </tbody>
      </table>,
    );
    expect(screen.getByText("permitRules.incompleteHint")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("permitRules.colPermitType"), {
      target: { value: "HOT_WORK" },
    });
    expect(onChange).toHaveBeenCalledWith({
      ...EMPTY_PERMIT_RULE_EDIT,
      permitTypeCode: "HOT_WORK",
    });
  });

  it("keeps a code no longer in its list visible as written", () => {
    render(
      <table>
        <tbody>
          <tr>
            <PermitRuleEditCells
              state={{ classCode: "OLD", workTypeCode: "", permitTypeCode: "HOT_WORK" }}
              onChange={vi.fn()}
              options={options}
            />
          </tr>
        </tbody>
      </table>,
    );
    expect(screen.getByLabelText("permitRules.colClass")).toHaveValue("OLD");
  });

  it("names the missing right instead of an empty permit type list", () => {
    render(
      <table>
        <tbody>
          <tr>
            <PermitRuleEditCells
              state={EMPTY_PERMIT_RULE_EDIT}
              onChange={vi.fn()}
              options={{ ...options, canReadPermitTypes: false, permitTypes: [] }}
            />
          </tr>
        </tbody>
      </table>,
    );
    expect(screen.getByTestId("list-access-permitTypes")).toBeInTheDocument();
    expect(screen.queryByLabelText("permitRules.colPermitType")).toBeNull();
  });
});

describe("usePermitRuleOptions", () => {
  function Probe({ enabled }: { enabled: boolean }) {
    const o = usePermitRuleOptions(enabled);
    return (
      <p data-testid="probe">
        {o.classes.map((c) => c.code).join(",")}|{o.workTypes.map((c) => c.code).join(",")}|
        {o.permitTypes.map((c) => c.code).join(",")}
      </p>
    );
  }

  it("reads the published classes and work types and the permit types, only when enabled", async () => {
    ref.listPublishedReferenceValuesByDomainCode.mockImplementation(async (code: string) =>
      code === "EQUIPMENT.CLASS"
        ? [{ code: "PUMP", label: "Pompes", is_active: true }]
        : [
            { code: "CORR", label: "Correctif", is_active: true },
            { code: "OLD", label: "Ancien", is_active: false },
          ],
    );
    permits.listPermitTypes.mockResolvedValue([{ code: "HOT_WORK", name: "Travail à chaud" }]);
    const { rerender } = render(<Probe enabled={false} />);
    expect(ref.listPublishedReferenceValuesByDomainCode).not.toHaveBeenCalled();
    rerender(<Probe enabled />);
    await waitFor(() =>
      expect(screen.getByTestId("probe")).toHaveTextContent("PUMP|CORR|HOT_WORK"),
    );
  });
});

describe("rules the conversion cannot use", () => {
  it("names the unknown class, work type or permit type, judged only once the lists are read", () => {
    const rule = { classCode: "GONE", workTypeCode: "", permitTypeCode: "HOT_WORK" };
    expect(permitRuleProblem(rule, options)).toEqual({ kind: "unknownClass", code: "GONE" });
    expect(permitRuleProblem(rule, { ...options, loaded: false })).toBeNull();
    expect(
      permitRuleProblem({ classCode: "PUMP", workTypeCode: "", permitTypeCode: "X" }, options),
    ).toEqual({ kind: "unknownPermitType", code: "X" });
    expect(
      permitRuleProblem(
        { classCode: "PUMP", workTypeCode: "", permitTypeCode: "HOT_WORK" },
        options,
      ),
    ).toBeNull();
  });

  it("lists the active rules that cannot be applied and marks them in the table", () => {
    const good = {
      ...value('{"equipment_class_code":"PUMP","permit_type_code":"HOT_WORK"}'),
      is_active: true,
    };
    const bad = {
      ...value('{"work_type_code":"OLD","permit_type_code":"HOT_WORK"}'),
      id: 2,
      code: "R2",
      is_active: true,
    };
    const off = { ...bad, id: 3, code: "R3", is_active: false };
    expect(unusablePermitRules([good, bad, off], options).map((v) => v.code)).toEqual(["R2"]);

    render(
      <table>
        <tbody>
          <tr>
            <PermitRuleReadCells value={bad} options={options} />
          </tr>
        </tbody>
      </table>,
    );
    expect(screen.getByText("permitRules.unusable")).toBeInTheDocument();
    expect(screen.getByText("permitRules.problem.unknownWorkType")).toBeInTheDocument();
  });
});
