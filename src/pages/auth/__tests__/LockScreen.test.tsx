/**
 * Lock screen error text: a coded failure shows its translated message; an uncoded failure keeps
 * the screen's own fallback. Display only — what is sent on unlock is unchanged.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";

import { i18n, initI18n } from "@/i18n/config";
import { LockScreen } from "@/pages/auth/LockScreen";

function renderLock(onUnlock: (password: string) => Promise<void>) {
  const { container } = render(
    <LockScreen displayName="Amina" onUnlock={onUnlock} onLogout={vi.fn()} />,
  );
  const input = container.querySelector('input[type="password"]') as HTMLInputElement;
  const form = input.closest("form") as HTMLFormElement;
  return { input, form };
}

describe("LockScreen error text", () => {
  beforeAll(async () => {
    initI18n();
    await i18n.changeLanguage("en");
  });

  it("shows the translated message of a coded failure and sends the password unchanged", async () => {
    const onUnlock = vi
      .fn()
      .mockRejectedValue({ code: "AUTH_ERROR", message: "mot de passe refusé (serveur)" });
    const { input, form } = renderLock(onUnlock);
    fireEvent.change(input, { target: { value: "s3cret!" } });
    fireEvent.submit(form);

    expect(
      await screen.findByText(String(i18n.t("errors:appError.AUTH_ERROR"))),
    ).toBeInTheDocument();
    expect(screen.queryByText("mot de passe refusé (serveur)")).not.toBeInTheDocument();
    expect(onUnlock).toHaveBeenCalledWith("s3cret!");
  });

  it("keeps the screen's fallback for an uncoded failure", async () => {
    const onUnlock = vi.fn().mockRejectedValue({});
    const { input, form } = renderLock(onUnlock);
    fireEvent.change(input, { target: { value: "x" } });
    fireEvent.submit(form);

    await waitFor(() => expect(onUnlock).toHaveBeenCalled());
    expect(
      await screen.findAllByText(String(i18n.t("auth:session.idleLocked.unlockAction"))),
    ).not.toHaveLength(0);
    expect(screen.queryByText(String(i18n.t("errors:generic.unexpected")))).not.toBeInTheDocument();
  });
});
