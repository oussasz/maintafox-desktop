import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { RouterProvider, createMemoryRouter, useNavigate } from "react-router-dom";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { DiCreateForm } from "@/components/di/DiCreateForm";
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
vi.mock("@/components/common/urgency/UrgencySelect", () => ({ UrgencySelect: () => null }));
vi.mock("@/components/common/urgency/use-urgency-scale", () => ({
  urgencyLevelLabel: () => "",
  useUrgencyScale: () => ({ levelOf: () => null, language: "fr" }),
}));
vi.mock("@/components/reference/ReferenceCombobox", () => ({ ReferenceCombobox: () => null }));
vi.mock("@/components/di/DiAttachmentPanel", () => ({ DiAttachmentPanel: () => null }));

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
  useDiStore.setState({ updateDraft } as unknown as Partial<
    ReturnType<typeof useDiStore.getState>
  >);
});

const initial = {
  id: 5,
  row_version: 3,
  title: "Fuite sur la pompe",
  description: "Fuite au niveau du joint",
  origin_type: "operator",
  request_type: "corrective",
  reported_urgency: "high",
  impact_level: "minor",
  symptom_code_id: 4,
  observed_at: null,
  safety_flag: false,
  environmental_flag: false,
  quality_flag: false,
  production_impact: false,
  equipment_id: 9,
} as unknown as InterventionRequest;

type FormProps = Partial<React.ComponentProps<typeof DiCreateForm>>;

function Screen({ onCancel, extra }: { onCancel: () => void; extra: FormProps }) {
  const navigate = useNavigate();
  return (
    <div>
      <button type="button" onClick={() => navigate("/elsewhere")}>
        leave-page
      </button>
      <DiCreateForm initial={initial} onSubmitted={vi.fn()} onCancel={onCancel} {...extra} />
    </div>
  );
}

function renderForm(onCancel = vi.fn(), extra: FormProps = {}) {
  const router = createMemoryRouter(
    [
      { path: "/di", element: <Screen onCancel={onCancel} extra={extra} /> },
      { path: "/elsewhere", element: <p>other page</p> },
    ],
    { initialEntries: ["/di"] },
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
  fireEvent.change(document.querySelector("#title") as HTMLInputElement, {
    target: { value },
  });

describe("DiCreateForm unsaved input", () => {
  it("does not ask when nothing was changed", async () => {
    renderForm();
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("asks before leaving the page once a field was changed", async () => {
    renderForm();
    typeTitle("Fuite importante sur la pompe");
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
    typeTitle("Fuite importante sur la pompe");
    const save = screen.getByText("form.update").closest("button") as HTMLButtonElement;
    await waitFor(() => expect(save).not.toBeDisabled());
    fireEvent.click(save);
    await waitFor(() => expect(updateDraft).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
    expect(screen.queryByText("discardGuard.title")).not.toBeInTheDocument();
  });
});

describe("DiCreateForm unsaved input in create mode with a prefilled equipment", () => {
  const prefillAsset = { id: 9, asset_code: "P-01", asset_name: "Pompe", org_node_id: 3 };

  it("does not ask when the form was opened with an equipment and left untouched", async () => {
    renderForm(vi.fn(), { initial: null, prefillAsset: prefillAsset as never });
    await settle();
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("asks once something is typed", async () => {
    renderForm(vi.fn(), { initial: null, prefillAsset: prefillAsset as never });
    await settle();
    typeTitle("Nouvelle demande");
    fireEvent.click(screen.getByRole("button", { name: "leave-page" }));
    expect(await screen.findByText("discardGuard.title")).toBeInTheDocument();
  });
});
