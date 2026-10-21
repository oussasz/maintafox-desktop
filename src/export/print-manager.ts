/**
 * PrintManager - official Maintafox print transport.
 *
 * Prints a self-contained HTML document from a hidden frame inside the main window, then removes
 * the frame. No second window is ever opened: the webview refuses new-window requests (wry
 * answers them with "handled" when the app registers no handler), so `window.open` and a
 * secondary Tauri window are not part of printing.
 *
 * The document is written into the frame (`document.write`), not loaded from a blob: URL or an
 * inline script, because the app CSP is `default-src 'self'; script-src 'self'` with no
 * `frame-src`. A frame created without a URL inherits the parent's origin and CSP.
 */

export interface PrintHtmlOptions {
  /** Title of the printed document (print header, print dialog). */
  title?: string;
}

/** The frame is removed when the print dialog reports it is finished, or after this long. */
export const PRINT_FRAME_FALLBACK_MS = 60_000;

/** Lets the written document be laid out before the print dialog snapshots it. */
const PAINT_FALLBACK_MS = 50;

/** A frame that never paints (window minimized right after the click) must not block printing. */
export const PRINT_PAINT_TIMEOUT_MS = 1000;

function nextPaint(frameWindow: Window): Promise<void> {
  return new Promise((resolve) => {
    // Settles on the first of: two animation frames, or the timeout (then print anyway).
    const timer = setTimeout(resolve, PRINT_PAINT_TIMEOUT_MS);
    const done = () => {
      clearTimeout(timer);
      resolve();
    };
    if (typeof frameWindow.requestAnimationFrame === "function") {
      frameWindow.requestAnimationFrame(() => {
        frameWindow.requestAnimationFrame(done);
      });
    } else {
      setTimeout(done, PAINT_FALLBACK_MS);
    }
  });
}

/** The print request in progress; a second request made meanwhile (double click) joins it. */
let inFlight: Promise<void> | null = null;
let inFlightHtml: string | null = null;

/**
 * Print a dedicated HTML document through a hidden frame of the current window.
 * Rejects when the frame cannot be created or the runtime has no `print()` for it.
 *
 * Only one print runs at a time: while one is being prepared or its print dialog is open, a
 * further call returns that same pending promise (the second document is NOT printed) so a
 * double click never appends two frames or calls `print()` twice.
 */
export function printHtml(html: string, options: PrintHtmlOptions = {}): Promise<void> {
  if (inFlight) {
    if (html !== inFlightHtml) {
      console.warn(
        "[PrintManager] A print is already open; this different document was not printed.",
      );
    }
    return inFlight;
  }
  const run = printInFrame(html, options).finally(() => {
    if (inFlight === run) {
      inFlight = null;
      inFlightHtml = null;
    }
  });
  inFlight = run;
  inFlightHtml = html;
  return run;
}

async function printInFrame(html: string, options: PrintHtmlOptions): Promise<void> {
  const title = options.title ?? "Print";

  const frame = document.createElement("iframe");
  frame.setAttribute("aria-hidden", "true");
  frame.title = title;
  frame.style.position = "fixed";
  frame.style.right = "0";
  frame.style.bottom = "0";
  frame.style.width = "0";
  frame.style.height = "0";
  frame.style.border = "0";
  document.body.appendChild(frame);

  let removed = false;
  let fallbackTimer: ReturnType<typeof setTimeout> | undefined;
  const remove = () => {
    if (removed) return;
    removed = true;
    clearTimeout(fallbackTimer);
    frame.remove();
  };

  const frameDoc = frame.contentDocument;
  const frameWindow = frame.contentWindow;
  if (!frameDoc || !frameWindow) {
    remove();
    throw new Error("PrintManager: unable to create the print frame.");
  }
  if (typeof frameWindow.print !== "function") {
    remove();
    throw new Error("PrintManager: printing is not available in this environment.");
  }

  frameDoc.open();
  frameDoc.write(html);
  frameDoc.close();
  try {
    frameDoc.title = title;
  } catch {
    // The title only labels the print job.
  }

  frameWindow.onafterprint = () => {
    clearTimeout(fallbackTimer);
    // Give the runtime a moment to finish handing the job over before the frame goes away.
    setTimeout(remove, 300);
  };
  // The runtime may never emit `afterprint`.
  fallbackTimer = setTimeout(remove, PRINT_FRAME_FALLBACK_MS);

  try {
    await nextPaint(frameWindow);
    frameWindow.focus();
    frameWindow.print();
  } catch (err) {
    remove();
    throw err;
  }
}
