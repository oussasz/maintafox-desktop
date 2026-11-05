import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RouterProvider, createMemoryRouter, useNavigate } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { WoCreateForm } from "@/components/wo/WoCreateForm";
import { useWoStore } from "@/stores/wo-store";
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

function Screen({ onCancel }: { onCancel: () => void }) {
  const navigate = useNavigate();
  return (
    <div>
      <button type="button" onClick={() => navigate("/elsewhere")}>
        leave-page
      </button>
      <WoCreateForm initial={initial} onSubmitted={vi.fn()} onCancel={onCancel} />
    </div>
  );
}

function renderForm(onCancel = vi.fn()) {
  const router = createMemoryRouter(
    [
      { path: "/ot", element: <Screen onCancel={onCancel} /> },
      { path: "/elsewhere", element: <p>other page</p> },
    ],
    { initialEntries: ["/ot"] },
  );
  render(<RouterProvider router={router} />);
  return { onCancel };
}

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
