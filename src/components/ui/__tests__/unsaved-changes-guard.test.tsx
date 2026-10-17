import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { RouterProvider, createMemoryRouter, useNavigate, useSearchParams } from "react-router-dom";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";

import { UnsavedChangesGuard } from "@/components/ui/unsaved-changes-guard";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
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

function Editor() {
  const [dirty, setDirty] = useState(false);
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  return (
    <div>
      <p>step {params.get("step") ?? "none"}</p>
      <button type="button" onClick={() => setDirty(true)}>
        edit
      </button>
      <button type="button" onClick={() => setParams({ step: "rcm" }, { replace: true })}>
        next step
      </button>
      <button type="button" onClick={() => navigate("/elsewhere")}>
        leave
      </button>
      <UnsavedChangesGuard
        when={dirty}
        title="Discard?"
        description="Changes are lost."
        confirmLabel="Discard changes"
      />
    </div>
  );
}

function renderInRouter() {
  const router = createMemoryRouter(
    [
      { path: "/study", element: <Editor /> },
      { path: "/elsewhere", element: <p>other page</p> },
    ],
    { initialEntries: ["/study?step=fmeca"] },
  );
  render(<RouterProvider router={router} />);
}

describe("UnsavedChangesGuard", () => {
  it("lets every navigation through while nothing is unsaved", async () => {
    renderInRouter();
    fireEvent.click(screen.getByRole("button", { name: "leave" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("stops a step change on unsaved work: Cancel stays, Discard goes on", async () => {
    renderInRouter();
    fireEvent.click(screen.getByRole("button", { name: "edit" }));

    fireEvent.click(screen.getByRole("button", { name: "next step" }));
    const ask = await screen.findByRole("dialog", { name: "Discard?" });
    fireEvent.click(within(ask).getByRole("button", { name: "ds.confirm.cancel" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByText("step fmeca")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "leave" }));
    const again = await screen.findByRole("dialog", { name: "Discard?" });
    fireEvent.click(within(again).getByRole("button", { name: "Discard changes" }));
    expect(await screen.findByText("other page")).toBeInTheDocument();
  });

  it("guards only the window outside a data router", () => {
    const add = vi.spyOn(window, "addEventListener");
    render(
      <UnsavedChangesGuard when title="Discard?" description="x" confirmLabel="Discard changes" />,
    );
    expect(add).toHaveBeenCalledWith("beforeunload", expect.any(Function));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    add.mockRestore();
  });
});
