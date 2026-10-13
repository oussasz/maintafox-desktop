; ============================================================================
; Maintafox installer / uninstaller branding: colours, icon and wording.
;
; Included by installer-hooks.nsh BEFORE the Tauri installer script declares its
; pages, so every MUI_* setting below is picked up by the page that follows.
; Edit this file to change texts or colours; edit scripts/generate-installer-art.py
; (and re-run `pnpm installer:art`) to change the images.
; See docs/engineering/INSTALLER_BRANDING.md.
; ============================================================================

; ---- Colours (hex RRGGBB, no prefix). Keep in sync with the art script. -----
; Header band, Welcome page and Finish page background / text colour.
!define MUI_BGCOLOR   "FFFFFF"
!define MUI_TEXTCOLOR "0B1F3A"

; ---- Uninstaller icon -------------------------------------------------------
; Installer icon, header and sidebar images are supplied by tauri.conf.json
; (nsis.installerIcon, nsis.headerImage, nsis.sidebarImage). The uninstaller
; header reuses the installer header image automatically; its icon has to be
; set here, otherwise uninstall.exe would carry NSIS's stock green/red icon.
; ${__FILEDIR__} is the folder of this file, so the path works wherever the
; project is checked out.
!define MUI_UNICON "${__FILEDIR__}\..\..\icons\icon.ico"

; ---- Language IDs used by the LangString entries below ----------------------
; (LANG_FRENCH / LANG_ENGLISH are not defined yet at this point of the script.)
!define BRAND_LANG_FR 1036
!define BRAND_LANG_EN 1033

; ---- Installer: Welcome page ------------------------------------------------
LangString brandWelcomeTitle ${BRAND_LANG_FR} "Bienvenue dans l'installation de Maintafox"
LangString brandWelcomeTitle ${BRAND_LANG_EN} "Welcome to the Maintafox setup"
LangString brandWelcomeText ${BRAND_LANG_FR} "Cet assistant va installer Maintafox, votre solution de GMAO, sur cet ordinateur.$\r$\n$\r$\nFermez les autres applications avant de continuer, puis cliquez sur Suivant."
LangString brandWelcomeText ${BRAND_LANG_EN} "This wizard will install Maintafox, your maintenance management (CMMS) solution, on this computer.$\r$\n$\r$\nClose other applications before you continue, then click Next."

; ---- Installer: Finish page -------------------------------------------------
LangString brandFinishTitle ${BRAND_LANG_FR} "Maintafox est installé"
LangString brandFinishTitle ${BRAND_LANG_EN} "Maintafox is installed"
LangString brandFinishText ${BRAND_LANG_FR} "L'installation est terminée.$\r$\n$\r$\nCliquez sur Terminer pour fermer l'assistant."
LangString brandFinishText ${BRAND_LANG_EN} "Setup is complete.$\r$\n$\r$\nClick Finish to close the wizard."
LangString brandRunApp ${BRAND_LANG_FR} "Lancer Maintafox"
LangString brandRunApp ${BRAND_LANG_EN} "Launch Maintafox"

; ---- Uninstaller: confirmation page -----------------------------------------
; Top text of the page. The "delete application data" check box below it is added
; by the Tauri installer script and labelled by Tauri's own language files.
LangString brandUnConfirmText ${BRAND_LANG_FR} "Vos données (base de données, sauvegardes) sont conservées, sauf si vous cochez la case ci-dessous."
LangString brandUnConfirmText ${BRAND_LANG_EN} "Your data (database, backups) is kept unless you tick the box below."
