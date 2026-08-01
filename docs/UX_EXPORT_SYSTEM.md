# UX Pattern: Maintafox Export & Print System

> **Status:** Adopted  
> **Package:** `src/export/`  
> **First consumer:** Asset QR / Identification label dialog  
> **Sibling patterns:** `UX_IDENTIFICATION_DIALOG_PATTERN.md`, `UX_ENTITY_FORM_DIALOG_PATTERN.md`

---

## Decision

Every printable or downloadable output uses the **same export philosophy**:

| Action | Transport |
|--------|-----------|
| Download PNG | Native **Save File** dialog → render → `writeFile` → success toast with path |
| Print | Temporary **OS window** (label only) → system print dialog → auto-close |
| Export PDF | `PdfExporter` (stub → Rust/reports adapter later) |

Never silently download. Never print inside the main application frame.

Modules supply **content**. The framework owns **transport**, **dialogs**, and **feedback**.

---

## Desktop UX rules

### Save (PNG / future PDF)

1. Open the OS Save dialog (default name e.g. `Asset_EQ-001_QR.png`, default folder Downloads).
2. If the user cancels → return cancelled; **no error toast**.
3. After confirm → generate bytes → write to the chosen path.
4. Success toast: title + absolute path + **Open folder** action.

### Print

1. Build a dedicated HTML document (no app chrome).
2. Open it in a **temporary native window** (Tauri `WebviewWindow`, blob window fallback).
3. Auto-trigger the OS print dialog from that window.
4. Close the temporary window after print/cancel (`onafterprint`).
5. Main Maintafox window stays on the same page — no navigation, no refresh.

---

## Public API

```ts
import {
  ExportActions,
  exportDocument,
  printHtml,
  revealInFolder,
  isExportCancelled,
  buildQrLabelHtml,
  renderQrLabelPngBlob,
} from "@/export";

const result = await exportDocument({
  format: "png",
  image: {
    filename: "Asset_EQ-001_QR.png",
    render: () => renderQrLabelPngBlob(model),
  },
});
// result.status === "saved" | throws ExportCancelledError
```

UI footer (everywhere):

1. **Fermer / Close** — outline  
2. **Imprimer / Print** — outline secondary  
3. **Télécharger PNG / Download PNG** — primary  

---

## Rules

- Do not call `window.print()` on the application page.
- Do not use empty `window.open("")` for print.
- Prefer source composition for PNG (canvas / structured data).
- Printable HTML must contain **only** the document — no buttons, dialogs, or overlays.
- Use `ExportActions` + `mfExport` for footer actions.

---

## Templates

| Id | Use |
|----|-----|
| `label-thermal` | Compact QR / identity labels (default for Asset QR) |
| `label-a4` | Same label content on A4 |
| `document-a4` | Future DI / WO / reports fiches |

---

## Follow-ups

- Migrate `DiPrintFiche` / `WoPrintFiche` → `printHtml`
- Wire `PdfExporter` to Rust `downloadExportedDocument`
- Reference Manager export UI → same Save dialog + toast pipeline
