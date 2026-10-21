import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from "vitest";

const webviewWindowCtor = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/webviewWindow", () => ({ WebviewWindow: webviewWindowCtor }));

import { PRINT_FRAME_FALLBACK_MS, printHtml } from "../print-manager";

interface FakeFrame {
  print: ReturnType<typeof vi.fn> | undefined;
  focus: ReturnType<typeof vi.fn>;
  onafterprint: (() => void) | null;
  requestAnimationFrame: (cb: () => void) => void;
}

function stubFrame(opts: { print?: boolean; doc?: boolean } = {}) {
  const win: FakeFrame = {
    print: opts.print === false ? undefined : vi.fn(),
    focus: vi.fn(),
    onafterprint: null,
    requestAnimationFrame: (cb) => cb(),
  };
  const doc = { open: vi.fn(), write: vi.fn(), close: vi.fn(), title: "" };
  vi.spyOn(HTMLIFrameElement.prototype, "contentWindow", "get").mockReturnValue(
    win as unknown as Window,
  );
  vi.spyOn(HTMLIFrameElement.prototype, "contentDocument", "get").mockReturnValue(
    opts.doc === false ? null : (doc as unknown as Document),
  );
  return { win, doc };
}

describe("printHtml", () => {
  let openSpy: MockInstance<typeof window.open>;
  beforeEach(() => {
    vi.useFakeTimers();
    openSpy = vi.spyOn(window, "open").mockReturnValue(null);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    document.body.innerHTML = "";
  });

  it("writes the document into a hidden frame of this window and prints that frame", async () => {
    const { win, doc } = stubFrame();
    await printHtml("<html><body>x</body></html>", { title: "Fiche" });

    const frame = document.querySelector("iframe") as HTMLIFrameElement;
    expect(frame).not.toBeNull();
    expect(frame.getAttribute("aria-hidden")).toBe("true");
    expect(doc.write).toHaveBeenCalledWith("<html><body>x</body></html>");
    expect(doc.title).toBe("Fiche");
    expect(win.print).toHaveBeenCalledTimes(1);
    expect(openSpy).not.toHaveBeenCalled();
    expect(webviewWindowCtor).not.toHaveBeenCalled();
  });

  it("removes the frame after afterprint", async () => {
    const { win } = stubFrame();
    await printHtml("<p>x</p>");
    expect(document.querySelector("iframe")).not.toBeNull();
    win.onafterprint?.();
    vi.advanceTimersByTime(400);
    expect(document.querySelector("iframe")).toBeNull();
  });

  it("removes the frame by timer when afterprint never comes", async () => {
    stubFrame();
    await printHtml("<p>x</p>");
    vi.advanceTimersByTime(PRINT_FRAME_FALLBACK_MS + 1);
    expect(document.querySelector("iframe")).toBeNull();
  });

  it("rejects and leaves no frame when the frame has no print()", async () => {
    stubFrame({ print: false });
    await expect(printHtml("<p>x</p>")).rejects.toThrow(/not available/);
    expect(document.querySelector("iframe")).toBeNull();
    expect(openSpy).not.toHaveBeenCalled();
    expect(webviewWindowCtor).not.toHaveBeenCalled();
  });

  it("rejects and leaves no frame when the frame has no document", async () => {
    stubFrame({ doc: false });
    await expect(printHtml("<p>x</p>")).rejects.toThrow(/print frame/);
    expect(document.querySelector("iframe")).toBeNull();
  });
});
