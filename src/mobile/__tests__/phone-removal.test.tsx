import { act, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import { useRemovalNotice } from "../hooks/removal-notice";
import { PhoneRemovalGate } from "../screens/PhoneRemovalScreen";
import type { PhoneRemovalStatus } from "../services/enrolment";

import { ipcAnswering, renderMobile } from "./helpers";

// The engine's event channel is the only thing replaced besides the IPC boundary.
const channel = vi.hoisted(() => ({
  handlers: [] as Array<(event: { payload: unknown }) => void>,
  unlisten: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name: string, handler: (event: { payload: unknown }) => void) => {
    channel.handlers.push(handler);
    return channel.unlisten;
  }),
}));

function status(overrides: Partial<PhoneRemovalStatus> = {}): PhoneRemovalStatus {
  return {
    phase: "none",
    kind: null,
    uploaded_count: 0,
    uploaded_by_kind: {},
    dropped_count: 0,
    dropped_by_kind: {},
    last_error: null,
    wipe_confirmed: null,
    ...overrides,
  };
}

function Enrolment() {
  const { oldSlotToRemove } = useRemovalNotice();
  return (
    <div>
      <p>ENROLMENT</p>
      {oldSlotToRemove ? <p>OLD SLOT</p> : null}
    </div>
  );
}

async function renderGate(initial: PhoneRemovalStatus) {
  mockInvoke.mockImplementation(ipcAnswering({ get_phone_removal_status: () => initial }));
  const view = renderMobile(
    <PhoneRemovalGate>
      <Enrolment />
    </PhoneRemovalGate>,
  );
  await act(async () => undefined);
  return view;
}

async function emit(next: PhoneRemovalStatus) {
  await act(async () => {
    for (const handler of channel.handlers) handler({ payload: next });
  });
}

describe("phone removal screen", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    channel.handlers.length = 0;
    channel.unlisten.mockClear();
  });

  it("a phone with no removal shows the app (the enrolment)", async () => {
    await renderGate(status());
    expect(screen.getByText("ENROLMENT")).toBeInTheDocument();
  });

  it("a removed phone uploads, then erases, live from the engine's events", async () => {
    await renderGate(status({ phase: "uploading", kind: "removed", uploaded_count: 3 }));
    expect(
      screen.getByText(
        "Ce téléphone a été retiré par votre administrateur. Envoi des travaux en attente…",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Travaux envoyés : 3")).toBeInTheDocument();
    expect(screen.queryByText("ENROLMENT")).toBeNull();

    await emit(status({ phase: "erasing", kind: "removed", uploaded_count: 7 }));
    expect(screen.getByText("Effacement des données…")).toBeInTheDocument();
    expect(screen.getByText("Travaux envoyés : 7")).toBeInTheDocument();
  });

  it("waiting to upload keeps the data, says a connection is needed and shows the failure", async () => {
    await renderGate(
      status({
        phase: "waiting_to_upload",
        kind: "removed",
        last_error: {
          code: "offline",
          kind: "retryable",
          message: "Cannot reach the control API",
          http_status: null,
          at: "2026-10-29T10:00:00Z",
        },
      }),
    );
    expect(
      screen.getByText(
        "Connexion nécessaire pour envoyer les travaux en attente. Les données sont conservées.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Serveur injoignable");
    expect(screen.queryByText(/Cannot reach/)).toBeNull();
  });

  it("an unbound phone must be enrolled by the administrator", async () => {
    await renderGate(status({ phase: "uploading", kind: "unbound" }));
    expect(
      screen.getByText(
        "Ce téléphone doit être enrôlé par votre administrateur. Envoi des travaux en attente…",
      ),
    ).toBeInTheDocument();
  });

  it("a revoked phone erases without sending and says what was not sent", async () => {
    await renderGate(
      status({
        phase: "erasing",
        kind: "revoked",
        dropped_count: 4,
        dropped_by_kind: { work_orders: 3, media_files: 1 },
      }),
    );
    expect(
      screen.getByText(
        "Ce téléphone a été déclaré perdu ou retiré : ses données sont effacées sans envoi.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Non envoyés : 4 (enregistrés dans le journal)")).toBeInTheDocument();
    expect(screen.getByText("dont photos non envoyées : 1")).toBeInTheDocument();
    expect(screen.queryByText(/Travaux envoyés/)).toBeNull();
  });

  it("photos not sent only show when there are some", async () => {
    await renderGate(
      status({ phase: "uploading", kind: "removed", dropped_count: 2, dropped_by_kind: { x: 2 } }),
    );
    expect(screen.getByText("Non envoyés : 2 (enregistrés dans le journal)")).toBeInTheDocument();
    expect(screen.queryByText(/photos non envoyées/)).toBeNull();
  });

  it("has no button that could cancel or skip the upload", async () => {
    for (const phase of ["uploading", "waiting_to_upload", "erasing"] as const) {
      const { unmount } = await renderGate(status({ phase, kind: "removed" }));
      expect(screen.queryAllByRole("button")).toEqual([]);
      expect(screen.queryAllByRole("link")).toEqual([]);
      unmount();
    }
  });

  it("when the erase is done the enrolment comes back", async () => {
    await renderGate(status({ phase: "erasing", kind: "removed" }));
    await emit(status({ phase: "done", kind: "removed", wipe_confirmed: true }));
    expect(screen.getByText("ENROLMENT")).toBeInTheDocument();
    expect(screen.queryByText("OLD SLOT")).toBeNull();
  });

  it("after an unbound phone's erase, the enrolment tells the administrator to free the old slot", async () => {
    await renderGate(status({ phase: "uploading", kind: "unbound" }));
    await emit(status({ phase: "done", kind: "unbound" }));
    expect(screen.getByText("ENROLMENT")).toBeInTheDocument();
    expect(screen.getByText("OLD SLOT")).toBeInTheDocument();
  });

  it("a removal that starts while the app is open takes over the screen", async () => {
    await renderGate(status());
    expect(screen.getByText("ENROLMENT")).toBeInTheDocument();
    await emit(status({ phase: "uploading", kind: "removed" }));
    expect(screen.queryByText("ENROLMENT")).toBeNull();
    expect(screen.getAllByText("Téléphone retiré").length).toBeGreaterThan(0);
  });

  it("photos sent are counted next to the work sent", async () => {
    await renderGate(
      status({
        phase: "uploading",
        kind: "removed",
        uploaded_count: 5,
        uploaded_by_kind: { work_orders: 3, media_files: 2 },
      }),
    );
    expect(screen.getByText("Travaux envoyés : 5")).toBeInTheDocument();
    expect(screen.getByText("dont photos envoyées : 2")).toBeInTheDocument();
  });

  it("a phone refused without confirmation is locked with its data kept, and erases nothing", async () => {
    await renderGate(status({ phase: "blocked", kind: "revoked_unconfirmed" }));
    expect(
      screen.getByText(
        "Ce téléphone est bloqué : le serveur l'a refusé sans confirmation. Les données sont conservées. Contactez votre administrateur.",
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText("Téléphone bloqué").length).toBeGreaterThan(0);
    expect(
      screen.getByText("Le serveur sera interrogé à nouveau régulièrement ; rien n'est effacé."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Effacement/)).toBeNull();
    expect(screen.queryByText("ENROLMENT")).toBeNull();
    expect(screen.queryAllByRole("button")).toEqual([]);

    // The server accepts the phone again: the lock is lifted, the app comes back.
    await emit(status());
    expect(screen.getByText("ENROLMENT")).toBeInTheDocument();
  });

  it("releases the event subscription on unmount", async () => {
    const { unmount } = await renderGate(status());
    unmount();
    expect(channel.unlisten).toHaveBeenCalledTimes(1);
  });
});
