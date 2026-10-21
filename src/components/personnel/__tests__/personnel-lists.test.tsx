/**
 * Personnel lists from Données de référence: values matched by exact code, look-alike names
 * shown with their code, "à revoir" values flagged, absences drawn by their behaviour tag, and
 * the "Statuts à revoir (N)" notice on the personnel page.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { absenceTimelineState } from "@/design-system/resource-timeline-tokens";
import {
  findPersonnelOption,
  personnelOptionTexts,
  usePersonnelListsStore,
} from "@/stores/personnel-lists-store";
import type { PersonnelOption, PersonnelReferenceOptions } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const svc = vi.hoisted(() => ({
  listPersonnelReferenceOptions: vi.fn(),
  listPersonnelStatusReview: vi.fn(),
}));
vi.mock("@/services/personnel-service", () => svc);

const perms = vi.hoisted(() => ({ granted: new Set<string>() }));
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({ can: (p: string) => perms.granted.has(p) }),
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "count" in opts ? `${key}:${String(opts["count"])}` : key,
    i18n: { language: "fr" },
  }),
}));

import { buildPlanningTimelineItems } from "../../planning/planning-model";
import { PersonnelListSelect } from "../PersonnelListSelect";
import { PersonnelStatusReviewNotice } from "../PersonnelStatusReviewNotice";
import { usePersonnelLists } from "../use-personnel-lists";

function opt(code: string, label: string, extra: Partial<PersonnelOption> = {}): PersonnelOption {
  return { code, label, label_en: null, tag: null, review_required: false, ...extra };
}

const LISTS: PersonnelReferenceOptions = {
  employment_statuses: [opt("active", "Actif"), opt("Actif", "Actif", { review_required: true })],
  employment_types: [opt("employee", "Salarié")],
  absence_types: [
    opt("leave", "Congé", { tag: "leave" }),
    opt("medical", "Médical", { tag: "critical" }),
    opt("stage", "Stage", { tag: "training" }),
  ],
  position_categories: [opt("technician", "Technicien")],
};

beforeEach(() => {
  vi.clearAllMocks();
  perms.granted = new Set();
  usePersonnelListsStore.setState({ options: null, loading: false, error: null });
});

describe("personnel list values", () => {
  it("matches codes exactly, never case-insensitively", () => {
    expect(findPersonnelOption(LISTS.employment_statuses, "Actif")?.review_required).toBe(true);
    expect(findPersonnelOption(LISTS.employment_statuses, "active")?.review_required).toBe(false);
    expect(findPersonnelOption(LISTS.employment_statuses, "ACTIVE")).toBeNull();
  });

  it("shows the code when two values of a list look alike", () => {
    const texts = personnelOptionTexts(LISTS.employment_statuses, "fr");
    expect(texts.get("active")).toBe("Actif (active)");
    expect(texts.get("Actif")).toBe("Actif (Actif)");
    expect(personnelOptionTexts(LISTS.employment_types, "fr").get("employee")).toBe("Salarié");
  });

  it("uses the English name in English when there is one", () => {
    const texts = personnelOptionTexts([opt("leave", "Congé", { label_en: "Leave" })], "en");
    expect(texts.get("leave")).toBe("Leave");
  });
});

describe("absence bars", () => {
  it("are drawn by the tag of their type", () => {
    expect(absenceTimelineState("leave", false)).toBe("leave");
    expect(absenceTimelineState("training", false)).toBe("training");
    expect(absenceTimelineState("critical", false)).toBe("critical");
    expect(absenceTimelineState(null, false)).toBe("blocked");
    expect(absenceTimelineState("leave", true)).toBe("critical");
  });

  it("on the planning board use the plant's names and tags, by exact code", () => {
    const lookup = (code: string) => {
      const v = findPersonnelOption(LISTS.absence_types, code);
      return v ? { text: v.label, tag: v.tag } : null;
    };
    const gantt = {
      assignee_lanes: [
        {
          personnel_id: 7,
          full_name: "Amine",
          blocked_intervals_json: JSON.stringify([
            {
              start_at: "2026-10-19T08:00:00Z",
              end_at: "2026-10-19T16:00:00Z",
              block_type: "stage",
              critical: false,
            },
            {
              start_at: "2026-10-20T08:00:00Z",
              end_at: "2026-10-20T16:00:00Z",
              block_type: "Leave",
              critical: false,
            },
            { start_at: "2026-10-21T08:00:00Z", end_at: "2026-10-21T16:00:00Z" },
          ]),
          commitments_json: "[]",
        },
      ],
    } as unknown as Parameters<typeof buildPlanningTimelineItems>[0]["gantt"];
    const items = buildPlanningTimelineItems({
      commitments: [],
      gantt,
      absenceType: lookup,
      absenceTag: (critical) => (critical ? "absent-critical" : "absent"),
    });
    expect(items.map((i) => [i.title, i.state])).toEqual([
      ["Stage", "training"],
      // "Leave" is not the code "leave": shown as stored, drawn as a plain absence.
      ["Leave", "blocked"],
      // No type (the caller has no per.view): the neutral tag, never assumed to be leave.
      ["absent", "blocked"],
    ]);
  });
});

describe("PersonnelStatusReviewNotice", () => {
  const rows = [
    { personnel_id: 4, employee_code: "E-004", full_name: "Karim B.", stored_status: "Actif" },
    { personnel_id: 9, employee_code: "E-009", full_name: "Sara L.", stored_status: "retired" },
  ];

  it("lists the people whose stored status needs review, with that status", async () => {
    perms.granted = new Set([P.PER_VIEW]);
    svc.listPersonnelStatusReview.mockResolvedValue(rows);
    const open = vi.fn();
    render(<PersonnelStatusReviewNotice onOpenPersonnel={open} />);

    const toggle = await screen.findByRole("button", { name: /statusReview.title:2/ });
    fireEvent.click(toggle);
    expect(screen.getByText("retired")).toBeInTheDocument();
    expect(screen.getByText("Actif")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Sara L\./ }));
    expect(open).toHaveBeenCalledWith(9);
  });

  it("is not shown without the personnel view right, and nothing is asked", () => {
    svc.listPersonnelStatusReview.mockResolvedValue(rows);
    const { container } = render(<PersonnelStatusReviewNotice onOpenPersonnel={vi.fn()} />);
    expect(container).toBeEmptyDOMElement();
    expect(svc.listPersonnelStatusReview).not.toHaveBeenCalled();
  });

  it("is not shown when nobody needs review", async () => {
    perms.granted = new Set([P.PER_VIEW]);
    svc.listPersonnelStatusReview.mockResolvedValue([]);
    const { container } = render(<PersonnelStatusReviewNotice onOpenPersonnel={vi.fn()} />);
    await waitFor(() => expect(svc.listPersonnelStatusReview).toHaveBeenCalled());
    expect(container).toBeEmptyDOMElement();
  });

  it("says when the list cannot be read, with a retry", async () => {
    perms.granted = new Set([P.PER_VIEW]);
    svc.listPersonnelStatusReview.mockRejectedValueOnce(new Error("boom"));
    render(<PersonnelStatusReviewNotice onOpenPersonnel={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("statusReview.loadError");
  });
});

describe("PersonnelListSelect", () => {
  function Field({ value }: { value: string }) {
    const lists = usePersonnelLists();
    return (
      <PersonnelListSelect
        list="employment_statuses"
        lists={lists}
        value={value}
        onChange={vi.fn()}
        ariaLabel="status"
      />
    );
  }

  beforeEach(() => {
    usePersonnelListsStore.setState({ options: LISTS, loading: false, error: null });
  });

  it("shows a usable value without a review mark", () => {
    render(<Field value="active" />);
    expect(screen.getByRole("combobox", { name: "status" })).toHaveTextContent("Actif (active)");
    expect(screen.queryByText("listValue.review")).toBeNull();
  });

  it("marks a value flagged for review", () => {
    render(<Field value="Actif" />);
    expect(screen.getByRole("combobox", { name: "status" })).toHaveTextContent("Actif (Actif)");
    expect(screen.getByText("listValue.review")).toBeInTheDocument();
  });

  it("keeps a stored code missing from the list visible, marked for review", () => {
    render(<Field value="retired" />);
    expect(screen.getByRole("combobox", { name: "status" })).toHaveTextContent("retired");
    expect(screen.getByText("listValue.review")).toBeInTheDocument();
  });

  it("says why the lists are missing when they cannot be read", async () => {
    usePersonnelListsStore.setState({ options: null, loading: false, error: null });
    svc.listPersonnelReferenceOptions.mockRejectedValueOnce(new Error("down"));
    render(<Field value="active" />);
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});
