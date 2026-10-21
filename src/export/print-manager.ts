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

function nextPaint(frameWindow: Window): Promise<void> {
  return new Promise((resolve) => {
    if (typeof frameWindow.requestAnimationFrame === "function") {
      frameWindow.requestAnimationFrame(() => {
        frameWindow.requestAnimationFrame(() => resolve());
      });
    } else {
      setTimeout(resolve, PAINT_FALLBACK_MS);
    }
  });
}

/**
 * Print a dedicated HTML document through a hidden frame of the current window.
 * Rejects when the frame cannot be created or the runtime has no `print()` for it.
 */
export async function printHtml(html: string, options: PrintHtmlOptions = {}): Promise<void> {
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
  const remove = () => {
    if (removed) return;
    removed = true;
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
    // Give the runtime a moment to finish handing the job over before the frame goes away.
    setTimeout(remove, 300);
  };
  // The runtime may never emit `afterprint`.
  setTimeout(remove, PRINT_FRAME_FALLBACK_MS);

  try {
    await nextPaint(frameWindow);
    frameWindow.focus();
    frameWindow.print();
  } catch (err) {
    remove();
    throw err;
  }
}
