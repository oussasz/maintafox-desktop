import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RouterProvider, createMemoryRouter, useNavigate } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { WoCreateForm } from "@/components/wo/WoCreateForm";
import { listWorkOrderSubtypes, listWorkOrderTypes } from "@/services/wo-service";
import { useWoStore } from "@/stores/wo-store";
import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import type { WorkOrder } from "@shared/ipc-types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "fr" } }),
}));
vi.mock("@/hooks/use-session", () => ({ useSession: () => ({ info: { user_id: 1 } }) }));
vi.mock("@/services/asset-service", () => ({ getAssetByIdSilent: vi.fn() }));
vi.mock("@/services/wo-service", () => ({
  listWorkOrderSubtypes: vi.fn(() => Promise.resolve([])),
  listWorkOrderTypes: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@/components/assets/AssetPicker", () => ({ AssetPicker: () => null }));
vi.mock("@/components/common/urgency/UrgencySelect", () => ({ UrgencySelect: () => null }));
vi.mock("@/components/wo/WoPermitRequirementField", () => ({
  WoPermitRequirementField: () => null,
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

const updateDraft = vi.fn(() => Promise.resolve());
beforeEach(() => {
  vi.clearAllMocks();
  useWoStore.setState({ updateDraft } as unknown as Partial<
    ReturnType<typeof useWoStore.getState>
  >);
});

const initial = {
  id: 5,
  row_version: 3,
  title: "Remplacer le disjoncteur",
  description: null,
  type_code: "",
  subtype_code: "",
  urgency_code: "",
  equipment_id: null,
  planned_start: null,
  planned_end: null,
  expected_duration_hours: null,
  notes: null,
  requires_permit: false,
} as unknown as WorkOrder;

type FormProps = Partial<React.ComponentProps<typeof WoCreateForm>>;

function Screen({ onCancel, extra }: { onCancel: () => void; extra: FormProps }) {
  const navigate = useNavigate();
  return (
    <div>
      <button type="button" onClick={() => navigate("/elsewhere")}>
        leave-page
      </button>
      <WoCreateForm initial={initial} onSubmitted={vi.fn()} onCancel={onCancel} {...extra} />
    </div>
  );
}

function renderForm(onCancel = vi.fn(), extra: FormProps = {}) {
  const router = createMemoryRouter(
    [
      { path: "/ot", element: <Screen onCancel={onCancel} extra={extra} /> },
      { path: "/elsewhere", element: <p>other page</p> },
    ],
    { initialEntries: ["/ot"] },
  );
  render(<RouterProvider router={router} />);
  return { onCancel };
}

/** Lets the form's loads (catalogs, equipment) land inside act. */
const settle = () =>
  act(async () => {
    await new Promise((r) => setTimeout(r, 50));
  });

const typeTitle = (value: string) =>
  fireEvent.change(document.querySelector("#wo-title") as HTMLInputElement, {
    target: { value },
  });

describe("WoCreateForm unsaved input", () => {
  it("does not ask when nothing was changed", async () => {
    renderForm();
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("asks before leaving the page once a field was changed", async () => {
    renderForm();
    typeTitle("Remplacer le disjoncteur Q2");
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("discardGuard.title")).toBeInTheDocument();
    expect(screen.queryByText("other page")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "discardGuard.leave" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("asks before the Cancel button drops what was typed, and closes at once when clean", async () => {
    const { onCancel } = renderForm();
    fireEvent.click(screen.getByRole("button", { name: "form.cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);

    typeTitle("Autre titre");
    fireEvent.click(screen.getByRole("button", { name: "form.cancel" }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    fireEvent.click(await screen.findByRole("button", { name: "discardGuard.leave" }));
    await waitFor(() => expect(onCancel).toHaveBeenCalledTimes(2));
  });

  it("does not ask after a successful save", async () => {
    renderForm();
    typeTitle("Remplacer le disjoncteur Q2");
    const save = screen.getByText("form.update").closest("button") as HTMLButtonElement;
    await waitFor(() => expect(save).not.toBeDisabled());
    fireEvent.click(save);
    await waitFor(() => expect(updateDraft).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
    expect(screen.queryByText("discardGuard.title")).not.toBeInTheDocument();
  });
});

const prefillAsset = { id: 7, asset_code: "P-07", asset_name: "Pompe 7", org_node_id: 3 };

describe("WoCreateForm unsaved input in create mode with a prefilled equipment", () => {
  beforeEach(() => {
    useWorkOrderTypesCatalog.setState({ types: [], loading: false, error: null });
    vi.mocked(listWorkOrderTypes).mockResolvedValue([
      { code: "corrective", label: "Correctif", nature: "corrective", is_active: true },
    ] as never);
  });

  it("does not ask when the form was opened with an equipment and left untouched", async () => {
    renderForm(vi.fn(), { initial: null, prefillAsset: prefillAsset as never });
    await waitFor(() => expect(listWorkOrderTypes).toHaveBeenCalled());
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("asks once something is typed", async () => {
    renderForm(vi.fn(), { initial: null, prefillAsset: prefillAsset as never });
    await settle();
    typeTitle("Nouvel OT");
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("discardGuard.title")).toBeInTheDocument();
  });
});

describe("WoCreateForm opening an edit that has a sub-type", () => {
  beforeEach(() => {
    useWorkOrderTypesCatalog.setState({ types: [], loading: false, error: null });
    vi.mocked(listWorkOrderTypes).mockResolvedValue([
      { code: "corrective", label: "Correctif", nature: "corrective", is_active: true },
    ] as never);
    vi.mocked(listWorkOrderSubtypes).mockResolvedValue([
      { code: "leak", label: "Fuite", nature: "corrective", is_active: true },
    ] as never);
  });

  it("keeps the sub-type and does not ask when nothing was touched", async () => {
    const withSubtype = { ...initial, type_code: "corrective", subtype_code: "leak" } as WorkOrder;
    renderForm(vi.fn(), { initial: withSubtype });
    await waitFor(() => expect(listWorkOrderSubtypes).toHaveBeenCalled());
    await settle();
    const save = screen.getByText("form.update").closest("button") as HTMLButtonElement;
    await waitFor(() => expect(save).not.toBeDisabled());
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
    expect(screen.queryByText("discardGuard.title")).not.toBeInTheDocument();
  });

  it("saves the stored sub-type unchanged", async () => {
    const withSubtype = { ...initial, type_code: "corrective", subtype_code: "leak" } as WorkOrder;
    renderForm(vi.fn(), { initial: withSubtype });
    await waitFor(() => expect(listWorkOrderSubtypes).toHaveBeenCalled());
    await settle();
    const save = screen.getByText("form.update").closest("button") as HTMLButtonElement;
    await waitFor(() => expect(save).not.toBeDisabled());
    fireEvent.click(save);
    await waitFor(() => expect(updateDraft).toHaveBeenCalledTimes(1));
    expect(updateDraft).toHaveBeenCalledWith(expect.objectContaining({ subtype_code: "leak" }));
  });
});
