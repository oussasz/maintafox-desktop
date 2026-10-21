# Windows installer branding

The Windows installer (`Maintafox_<version>_x64-setup.exe`, built by Tauri with NSIS) is a
**branded standard wizard**: the reliable Windows setup wizard (NSIS Modern UI 2) with Maintafox
artwork, colours, icon and French / English wording. It is deliberately not a custom-drawn window.

## What is branded

| Where | What |
| --- | --- |
| Language dialog (first launch) | French (default) / English. The choice is remembered and reused by the uninstaller and by updates. |
| Welcome and Finish pages | Sidebar image (navy to blue gradient, white logo, wordmark, tagline), custom title/text, text colour navy `#0B1F3A`, "Lancer Maintafox" / "Launch Maintafox" check box. |
| All other installer pages | Header strip with the colour logo and wordmark on white. |
| Uninstaller | Header strip, Maintafox icon on `uninstall.exe` (instead of the NSIS stock icon), confirmation page stating that data is kept unless the box is ticked. |
| Windows "Apps" list | Publisher "Maintafox Systems", homepage, copyright. |
| Start menu | Folder `Maintafox`. |

Unchanged on purpose (Tauri behaviour): the *Delete the application data* check box on the uninstall
confirmation page (removes `%APPDATA%\systems.maintafox.desktop` and `%LOCALAPPDATA%\systems.maintafox.desktop`,
including the database and backups, only when ticked), per-user install into `%LOCALAPPDATA%\Maintafox`
(no administrator rights), silent / passive modes used by the auto-updater, shortcut handling.

## Files

| File | Role |
| --- | --- |
| `src-tauri/tauri.conf.json` (`bundle.publisher`, `homepage`, `copyright`, `shortDescription`, `longDescription`, `bundle.windows.nsis.*`) | Official Tauri options: images, icon, languages, language selector, install mode, Start menu folder, hooks file. |
| `src-tauri/windows/nsis/installer-hooks.nsh` | Entry point included by Tauri in the generated script. Sets the page wording. |
| `src-tauri/windows/nsis/branding.nsh` | Colours, uninstaller icon, all French / English texts. |
| `src-tauri/windows/nsis/sidebar.bmp` (164x314), `header.bmp` (150x57) | Generated artwork (24-bit BMP, the only format NSIS accepts). |
| `scripts/generate-installer-art.py` | Generates both bitmaps and the PNG previews. |
| `docs/engineering/installer-preview/*.png` | Previews of the two bitmaps (not screenshots of the wizard). |

## Regenerate the artwork

```powershell
pnpm installer:art
```

Needs Python with Pillow (`pip install pillow`). The script uses the logos in `src/assets/logo/`,
Segoe UI from `C:\Windows\Fonts` (it prints a warning if it has to fall back to another font),
renders at 4x and reduces with LANCZOS for crisp edges. Commit the regenerated `.bmp` and `.png` files.

## Change colours, texts, language

- Image colours, wordmark, tagline: constants at the top of `scripts/generate-installer-art.py`, then `pnpm installer:art`.
- Page colours (`MUI_BGCOLOR`, `MUI_TEXTCOLOR`) and all wording: `src-tauri/windows/nsis/branding.nsh`.
  Keep the header bitmap background equal to `MUI_BGCOLOR` (white) or the header band shows a seam.
- Add a language: add it to `bundle.windows.nsis.languages` and add a `LangString` line for each text in
  `branding.nsh` using the Windows language id (1036 French, 1033 English, 1031 German, ...). A missing string makes makensis print a warning and the text shows blank.
- Do not declare pages (`MUI_PAGE_*`) in the hooks file: the first page declaration freezes the icon and
  header settings that Tauri defines later in the script.

## Test

1. Quick syntax check without compiling the app: generate the installer script with Tauri (it is written to
   `src-tauri/target/release/nsis/x64/installer.nsi` by `pnpm tauri build`) and run
   `%LOCALAPPDATA%\tauri\NSIS\makensis.exe installer.nsi`. Errors (bad define, missing image, unreadable text) show immediately.
2. Full check: `pnpm tauri build`, then install the produced `...-setup.exe` **in a virtual machine or a spare
   Windows account**, once in French and once in English (language dialog), and walk through every page:
   language, welcome, (reinstall page if already installed), folder, Start menu folder, install, finish + launch;
   then uninstall from Windows Settings > Apps with and without *Delete the application data* and confirm the data folder is kept / removed.
3. Update check: install an older build, then run the auto-update; the wizard must stay hidden/passive and keep the language.

## Known limits (honest)

- **Unsigned installer**: Windows SmartScreen shows "Windows protected your PC" until the installer is signed with a code-signing certificate (not part of this work).
- **It remains a Windows wizard.** Buttons, the folder/Start-menu/progress pages and fonts use native Windows controls and cannot be re-skinned without a plugin-based custom window (explicitly not wanted). Only the header, sidebar, colours of header/welcome/finish and wording are branded.
- **No sidebar on the uninstaller.** Tauri's uninstaller has only a confirmation page and a progress page (no Welcome/Finish page), and a page cannot be added from a hooks file. It gets the header strip, icon and branded text. A sidebar there would require maintaining a full copy of Tauri's NSIS template (`nsis.template`), which then has to be merged by hand on every Tauri upgrade; not done.
- **Sidebar sharpness**: the 164x314 bitmap is stretched by Windows on high-DPI screens (125% and above), so it is slightly softer there.
- **Publisher change**: Tauri derives the registry key `HKCU\Software\<publisher>\Maintafox` (remembered install folder and language) from `bundle.publisher`. Machines that installed an older, un-branded build (key `Software\maintafox`) lose that remembered folder on the next manual install; the default folder is identical, and auto-updates are unaffected. There is no production install base at the time of writing.
- `bundle.homepage` (`https://maintafox.systems`) is shown in the Apps list; change it if the public site differs.
