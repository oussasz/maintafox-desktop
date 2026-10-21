import { type MockInstance, afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const webviewWindowCtor = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/webviewWindow", () => ({ WebviewWindow: webviewWindowCtor }));

import { PRINT_FRAME_FALLBACK_MS, PRINT_PAINT_TIMEOUT_MS, printHtml } from "../print-manager";

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

  it("a second request while one is open joins it: one frame, one print()", async () => {
    const { win } = stubFrame();
    const first = printHtml("<p>a</p>");
    const second = printHtml("<p>b</p>");
    expect(second).toBe(first);
    await Promise.all([first, second]);
    expect(document.querySelectorAll("iframe")).toHaveLength(1);
    expect(win.print).toHaveBeenCalledTimes(1);
    // once finished, a new request prints again
    await printHtml("<p>c</p>");
    expect(win.print).toHaveBeenCalledTimes(2);
  });

  it("removes the frame and rejects when print() throws, and allows a retry", async () => {
    const { win } = stubFrame();
    (win.print as ReturnType<typeof vi.fn>).mockImplementationOnce(() => {
      throw new Error("print boom");
    });
    await expect(printHtml("<p>x</p>")).rejects.toThrow("print boom");
    expect(document.querySelector("iframe")).toBeNull();
    await printHtml("<p>y</p>");
    expect(win.print).toHaveBeenCalledTimes(2);
  });

  it("still prints after the timeout when the frame never paints, and does not stay locked", async () => {
    const { win } = stubFrame();
    win.requestAnimationFrame = () => {
      // never fires
    };
    const pending = printHtml("<p>x</p>");
    await vi.advanceTimersByTimeAsync(PRINT_PAINT_TIMEOUT_MS + 1);
    await pending;
    expect(win.print).toHaveBeenCalledTimes(1);

    win.requestAnimationFrame = (cb) => cb();
    await printHtml("<p>y</p>");
    expect(win.print).toHaveBeenCalledTimes(2);
  });

  it("warns when a joined request carries a different document", async () => {
    stubFrame();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const first = printHtml("<p>a</p>");
    void printHtml("<p>a</p>");
    expect(warn).not.toHaveBeenCalled();
    void printHtml("<p>b</p>");
    expect(warn).toHaveBeenCalledTimes(1);
    await first;
  });
});
