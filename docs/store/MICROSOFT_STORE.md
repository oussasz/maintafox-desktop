# Microsoft Store distribution

Maintafox reaches Windows users two ways, built from the same code:

| | Direct download (`release.yml`) | Microsoft Store (`store.yml`) |
|---|---|---|
| Package | NSIS installer `Maintafox_X.Y.Z_x64-setup.exe` | MSIX `Maintafox_X.Y.Z_x64.msix` / `.msixupload` |
| Signed by | not code-signed (SmartScreen warns); minisign for the updater | the Microsoft Store (owner decision 2026-10-24: no certificate) |
| Updates | in-app updater, control API manifest, rings internal / pilot / stable | the Store; in-app updater absent (`MAINTAFOX_DISTRIBUTION=store`) |
| Data folder | `%APPDATA%\systems.maintafox.desktop` | the same folder (write virtualisation off) |

## Store flavour

`MAINTAFOX_DISTRIBUTION=store` at build time (`src-tauri/build.rs`):

- no updater key is embedded and the updater plugin is not registered (`updater::STORE_DISTRIBUTION`);
- an update check answers `store_managed`: the update screens say the Store installs updates
  (Microsoft Store > Library > Get updates); the licence's forced-update screen says the same;
- before migrating an existing database to a new version, the app backs it up itself (a
  `pre_update` run, file `maintafox-pre-store-update-<version>-<time>.db` in the backup folder),
  because the Store replaced the app without the in-app updater's pre-update backup. If that
  backup fails, the database is not touched and the start-up screen says why.

Everything else is the direct-download app: same identifier, data folder, OS credential store
entries (database key, device secret, activation), device fingerprint (MachineGuid, host name, OS).
So activation and data survive Store updates, and a PC moving from the installer build to the
Store build keeps its data and activation (uninstall the installer build first, keeping its data).

## Package

- Manifest: `src-tauri/windows/msix/AppxManifest.template.xml` (full-trust desktop app,
  `runFullTrust`, `internetClient`, `unvirtualizedResources` with
  `desktop6:FileSystemWriteVirtualization` disabled; Windows 10 1903 or later, x64).
- Images: `src-tauri/windows/msix/Assets`, made from `src-tauri/icons/app-icon.png` by
  `scripts/store/make-store-assets.ps1`; Store listing logos in `docs/store/listing`.
- Packing: `scripts/store/build-msix.ps1` (Windows SDK `makepri` + `makeappx`; unsigned).
- CI: `.github/workflows/store.yml`. Identity values are repository variables
  `MSSTORE_IDENTITY_NAME`, `MSSTORE_PUBLISHER`, `MSSTORE_PUBLISHER_DISPLAY_NAME` (Partner Center >
  the app > Product management > Product identity). On a release tag it runs once they are set; a
  pull request touching the packaging runs a packaging check with a test identity (artifact named
  NOT-FOR-STORE).

## Known limits

- **No rollback from the Store.** The Store cannot install an older version. A failed database
  update on a Store PC stops at the start-up error with the pre-update backup kept; once a fixed
  version is in the Store, Maintafox support puts that backup back if the data needs it.
- **Rings.** Release rings (internal / pilot / stable) decide what the direct-download build is
  offered. Store PCs get what the Store submission publishes; use Partner Center package flights
  for internal / pilot testers. Licence entitlements (features, seats, expiry) are signed by the
  licence server and apply to both builds the same way.
- **Forced update floor.** Raise a licence's minimum version only after the Store shows that
  version as published (Store certification takes up to a few working days), or Store PCs stay
  blocked until it is.
- **WebView2.** An MSIX cannot run the WebView2 installer. Windows 11 and up-to-date Windows 10
  include the WebView2 runtime; a PC without it must install it from Microsoft before Maintafox
  starts.
- **To check on the first Store test install:** Windows notifications (toast) from the packaged
  app, and the `unvirtualizedResources` capability being accepted at certification (Partner Center
  asks why: "CMMS database and attachments must survive app updates and reinstalls, and be shared
  with the direct-download edition").
