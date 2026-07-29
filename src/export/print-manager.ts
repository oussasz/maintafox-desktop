/**
 * PrintManager — official Maintafox print transport.
 *
 * Opens a dedicated temporary OS window with only the printable document,
 * triggers the system print dialog, then closes that window.
 * Never prints from (or replaces) the main application UI.
 */

export interface PrintHtmlOptions {
  /** Window title for the temporary print surface. */
  title?: string;
}

function injectPrintLifecycle(html: string): string {
  const script = `<script>
(function () {
  function triggerPrint() {
    requestAnimationFrame(function () {
      requestAnimationFrame(function () {
        try { window.focus(); } catch (e) {}
        window.print();
      });
    });
  }
  if (document.readyState === "complete") triggerPrint();
  else window.addEventListener("load", triggerPrint);
  window.onafterprint = function () {
    setTimeout(function () { try { window.close(); } catch (e) {} }, 150);
  };
  setTimeout(function () { try { window.close(); } catch (e) {} }, 60000);
})();
</script>`;

  if (html.includes("</body>")) {
    return html.replace("</body>", `${script}</body>`);
  }
  return `${html}${script}`;
}

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Print a dedicated HTML document in a temporary native window.
 * The main Maintafox window is left unchanged.
 */
export async function printHtml(html: string, options: PrintHtmlOptions = {}): Promise<void> {
  const documentHtml = injectPrintLifecycle(html);
  const title = options.title ?? "Print";

  // Blob window: separate OS window, inline print script runs (CSP-safe vs asset protocol).
  try {
    await printViaBlobWindow(documentHtml);
    return;
  } catch (blobErr) {
    if (!isTauriRuntime()) throw blobErr;
    console.warn("[PrintManager] Blob print window failed; trying Tauri WebviewWindow.", blobErr);
  }

  await printViaTauriWindow(documentHtml, title);
}

async function printViaBlobWindow(html: string): Promise<void> {
  const blob = new Blob([html], { type: "text/html;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const printWindow = window.open(url, "_blank", "noopener,noreferrer,width=480,height=680");

  if (!printWindow) {
    URL.revokeObjectURL(url);
    throw new Error(
      "PrintManager: unable to open a temporary print window. Check popup permissions.",
    );
  }

  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

async function printViaTauriWindow(html: string, title: string): Promise<void> {
  const { tempDir, join } = await import("@tauri-apps/api/path");
  const { writeTextFile, remove } = await import("@tauri-apps/plugin-fs");
  const { convertFileSrc } = await import("@tauri-apps/api/core");
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");

  const dir = await tempDir();
  const fileName = `maintafox-print-${Date.now()}.html`;
  const filePath = await join(dir, fileName);
  await writeTextFile(filePath, html);

  const label = `print-${Date.now()}`;
  const url = convertFileSrc(filePath);

  const win = new WebviewWindow(label, {
    url,
    title,
    width: 480,
    height: 680,
    center: true,
    focus: true,
    resizable: true,
    decorations: true,
    visible: true,
  });

  await new Promise<void>((resolve, reject) => {
    void win.once("tauri://created", () => resolve());
    void win.once("tauri://error", (e) => {
      reject(e instanceof Error ? e : new Error(String(e)));
    });
  });

  setTimeout(() => {
    void remove(filePath).catch(() => {});
  }, 120_000);
}
