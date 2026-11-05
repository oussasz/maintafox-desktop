import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { revealFirstFormError } from "../reveal-first-form-error";

describe("revealFirstFormError", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    Element.prototype.scrollIntoView = vi.fn();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = "";
  });

  it("scrolls to the first field error in the form and focuses its control", () => {
    document.body.innerHTML = `
      <form id="f">
        <input id="asset_code" />
        <button id="criticality_code" type="button">Choisir</button>
        <p id="criticality_code-error" role="alert">Obligatoire</p>
        <input id="status_code" />
        <p id="status_code-error" role="alert">Obligatoire</p>
      </form>`;

    revealFirstFormError("f");
    expect(document.activeElement).toBe(document.body);
    vi.runAllTimers();

    const alert = document.getElementById("criticality_code-error");
    expect(alert?.scrollIntoView).toHaveBeenCalledWith({ block: "center" });
    expect(document.activeElement).toBe(document.getElementById("criticality_code"));
  });

  it("only scrolls to a form-level alert that has no control", () => {
    document.body.innerHTML = `
      <form id="f">
        <input id="asset_code" />
        <div role="alert">Le site est obligatoire</div>
      </form>`;

    revealFirstFormError("f");
    vi.runAllTimers();

    expect(Element.prototype.scrollIntoView).toHaveBeenCalledTimes(1);
    expect(document.activeElement).toBe(document.body);
  });

  it("ignores alerts outside the form and does nothing without errors", () => {
    document.body.innerHTML = `
      <div role="alert">Erreur de chargement</div>
      <form id="f"><input id="asset_code" /></form>`;

    revealFirstFormError("f");
    vi.runAllTimers();

    expect(Element.prototype.scrollIntoView).not.toHaveBeenCalled();
  });
});
