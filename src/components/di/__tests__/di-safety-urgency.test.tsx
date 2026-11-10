// L4-11 (CMMS Advisor decision, 2026-11-06): a request flagged "sécurité" is urgent at least. The
// request form chooses "Urgent" when the flag is ticked (a more urgent level stays), offers no less
// urgent level and says why; a request saved flagged cannot be unticked; the screening dialog asks for the review reason before a flagged
// request is validated below "Urgent".

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactElement } from "react";
import { RouterProvider, createMemoryRouter } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { DiCreateForm } from "@/components/di/DiCreateForm";
import { DiScreenDialog } from "@/components/di/DiScreenDialog";
import { useDiStore } from "@/stores/di-store";
import type { InterventionRequest } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 1 } }) }));
vi.mock("@/services/asset-service", () => ({
  getAssetById: vi.fn(() =>
    Promise.resolve({ id: 9, asset_code: "P-01", asset_name: "Pompe", org_node_id: 3 }),
  ),
  getAssetByIdSilent: vi.fn(),
}));
vi.mock("@/services/di-service", () => ({
  findPossibleDuplicateDis: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@/services/di-attachment-service", () => ({ uploadDiAttachmentFromPath: vi.fn() }));
vi.mock("@/components/assets/AssetPicker", () => ({ AssetPicker: () => null }));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));
vi.mock("@/components/di/DiAttachmentPanel", () => ({ DiAttachmentPanel: () => null }));

/** The shared urgency scale (`WORK.URGENCY`): codes and ranks fixed by the product. */
const RANKS: Record<string, number> = { emergency: 1, urgent: 2, high: 3, normal: 4, low: 5 };
vi.mock("@/components/common/urgency/use-urgency-scale", () => ({
  urgencyLevelLabel: () => "",
  useUrgencyScale: () => ({
    levelOf: (code: string | null | undefined) =>
      code && RANKS[code] ? { code, rank: RANKS[code], description: "" } : null,
    rankOf: (code: string | null | undefined) => (code && RANKS[code]) || Number.MAX_SAFE_INTEGER,
    labelOf: (code: string | null | undefined) => code ?? "",
    language: "fr",
  }),
}));

/** The select as the form drives it: its value and the least urgent rank it may offer. */
vi.mock("@/components/common/urgency/UrgencySelect", () => ({
  UrgencySelect: ({
    id,
    value,
    onChange,
    leastUrgentRank,
  }: {
    id?: string;
    value: string | null;
    onChange: (code: string) => void;
    leastUrgentRank?: number;
  }) => (
    <div data-testid={`select-${id}`} data-value={value ?? ""}>
      {Object.entries(RANKS).map(([code, rank]) => (
        <button
          key={code}
          type="button"
          disabled={leastUrgentRank != null && rank > leastUrgentRank}
          onClick={() => onChange(code)}
        >
          {`${id}:${code}`}
        </button>
      ))}
    </div>
  ),
}));

const store = vi.hoisted(() => ({ screen: vi.fn() }));
vi.mock("@/stores/di-review-store", () => ({
  useDiReviewStore: (sel: (s: typeof store) => unknown) => sel(store),
}));
vi.mock("@/services/reference-service", () => ({
  listPublishedReferenceValuesByDomainCode: vi.fn(() => Promise.resolve([])),
}));

// The data router builds a fetch Request per navigation; jsdom's AbortSignal is not the one
// Node's Request accepts, so the signal is dropped here (no loaders run in these routes).
const NativeRequest = globalThis.Request;
beforeAll(() => {
  globalThis.Request = class extends NativeRequest {
    constructor(input: RequestInfo | URL, init?: RequestInit) {
      const rest = { ...init };
      delete rest.signal;
      super(input, rest);
    }
  } as typeof Request;
});
afterAll(() => {
  globalThis.Request = NativeRequest;
});

/** The form guards unsaved input with the router's blocker: rendered in a data router. */
function renderRouted(element: ReactElement) {
  const router = createMemoryRouter([{ path: "/di", element }], { initialEntries: ["/di"] });
  return render(<RouterProvider router={router} />);
}

const updateDraft = vi.fn(() => Promise.resolve());
beforeEach(() => {
  vi.clearAllMocks();
  useDiStore.setState({ updateDraft } as unknown as Partial<
    ReturnType<typeof useDiStore.getState>
  >);
});

function request(reported: string, safety = false): InterventionRequest {
  return {
    id: 5,
    code: "DI-0005",
    row_version: 3,
    status: "submitted",
    title: "Fuite de vapeur",
    description: "Fuite sur la bride",
    origin_type: "operator",
    request_type: "repair",
    reported_urgency: reported,
    validated_urgency: null,
    impact_level: "minor",
    symptom_code_id: 4,
    classification_code_id: null,
    reviewer_note: null,
    observed_at: null,
    safety_flag: safety,
    environmental_flag: false,
    quality_flag: false,
    production_impact: false,
    equipment_id: 9,
  } as unknown as InterventionRequest;
}

const settle = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });

const urgencyValue = () => screen.getByTestId("select-reported_urgency").getAttribute("data-value");

describe("DiCreateForm — safety flag", () => {
  it("ticking the flag chooses Urgent, offers no less urgent level and says why", async () => {
    renderRouted(
      <DiCreateForm initial={request("high")} onSubmitted={vi.fn()} onCancel={vi.fn()} />,
    );
    await settle();
    expect(urgencyValue()).toBe("high");
    expect(screen.queryByTestId("di-safety-urgency-hint")).not.toBeInTheDocument();

    fireEvent.click(document.querySelector("#safety_flag") as HTMLElement);

    await waitFor(() => expect(urgencyValue()).toBe("urgent"));
    expect(screen.getByTestId("di-safety-urgency-hint")).toHaveTextContent(
      "form.safetyFlag.minUrgencyHint",
    );
    expect(screen.getByRole("button", { name: "reported_urgency:emergency" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "reported_urgency:urgent" })).toBeEnabled();
    for (const lower of ["high", "normal", "low"]) {
      expect(screen.getByRole("button", { name: `reported_urgency:${lower}` })).toBeDisabled();
    }

    const save = screen.getByText("form.update").closest("button") as HTMLButtonElement;
    await waitFor(() => expect(save).not.toBeDisabled());
    fireEvent.click(save);
    await waitFor(() => expect(updateDraft).toHaveBeenCalledTimes(1));
    expect(updateDraft).toHaveBeenCalledWith(
      expect.objectContaining({ safety_flag: true, reported_urgency: "urgent" }),
    );
  });

  it("keeps Emergency when the flag is ticked", async () => {
    renderRouted(
      <DiCreateForm initial={request("emergency")} onSubmitted={vi.fn()} onCancel={vi.fn()} />,
    );
    await settle();
    fireEvent.click(document.querySelector("#safety_flag") as HTMLElement);
    await screen.findByTestId("di-safety-urgency-hint");
    expect(urgencyValue()).toBe("emergency");
  });

  it("a request saved flagged cannot be unticked and says why", async () => {
    renderRouted(
      <DiCreateForm initial={request("urgent", true)} onSubmitted={vi.fn()} onCancel={vi.fn()} />,
    );
    await settle();
    const box = document.querySelector("#safety_flag") as HTMLButtonElement;
    expect(box).toBeDisabled();
    expect(screen.getByTestId("di-safety-flag-locked-hint")).toHaveTextContent(
      "form.safetyFlag.lockedHint",
    );
    fireEvent.click(box);
    await settle();
    expect(box).toBeDisabled();
    expect(screen.getByTestId("di-safety-urgency-hint")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "reported_urgency:low" })).toBeDisabled();
  });

  it("an unflagged saved request shows no lock hint", async () => {
    renderRouted(
      <DiCreateForm initial={request("high")} onSubmitted={vi.fn()} onCancel={vi.fn()} />,
    );
    await settle();
    expect(document.querySelector("#safety_flag")).toBeEnabled();
    expect(screen.queryByTestId("di-safety-flag-locked-hint")).not.toBeInTheDocument();
  });
});

describe("DiScreenDialog — flagged request validated below Urgent", () => {
  it("asks for the review reason before sending, and sends it", async () => {
    store.screen.mockResolvedValue({ ...request("normal", true), status: "awaiting_approval" });
    const di = { ...request("urgent", true), status: "in_review" } as InterventionRequest;
    render(<DiScreenDialog di={di} open onOpenChange={vi.fn()} />);
    await settle();
    const submit = screen.getByRole("button", { name: "screening.submit" });
    expect(submit).toBeEnabled();
    expect(screen.queryByTestId("di-screen-safety-reason")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "di-screen-urgency:normal" }));
    expect(await screen.findByTestId("di-screen-safety-reason")).toHaveTextContent(
      "screening.safetyBelowUrgent",
    );
    expect(screen.getByText("screening.noteRequired")).toBeInTheDocument();
    expect(submit).toBeDisabled();

    fireEvent.change(document.querySelector("#di-screen-note") as HTMLTextAreaElement, {
      target: { value: "Fuite isolée et balisée" },
    });
    expect(submit).toBeEnabled();
    fireEvent.click(submit);
    await waitFor(() => expect(store.screen).toHaveBeenCalledTimes(1));
    expect(store.screen).toHaveBeenCalledWith(
      expect.objectContaining({
        validated_urgency: "normal",
        reviewer_note: "Fuite isolée et balisée",
      }),
    );
  });

  it("needs no reason for an unflagged request", async () => {
    render(
      <DiScreenDialog
        di={{ ...request("urgent"), status: "in_review" } as InterventionRequest}
        open
        onOpenChange={vi.fn()}
      />,
    );
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "di-screen-urgency:low" }));
    expect(screen.queryByTestId("di-screen-safety-reason")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "screening.submit" })).toBeEnabled();
  });
});
