# Update safety and rollback (desktop, master plan Phase 5.5)

## Before every update
The running version backs up the database (`backups/maintafox-pre-update-<from>-to-<to>-<time>.db`,
encrypted like the database, recorded in the backup history as `pre_update`) and writes
`update-journal.json` next to the database. Then it asks the server again (a recall or pause since
the last check stops the install), downloads the installer, checks its updater signature against
the key built into the app, and runs it (the app restarts).

## Database migrations are forward-only
An older version cannot open a database that a newer version has migrated. So:

- **The new version cannot update the database** (first start after the update): it stages the
  pre-update backup (the Phase 1.3 restore: integrity and key checked, the half-updated file kept
  as a `pre_restore` backup) and shows "The update could not be completed" with a button that
  reinstalls the previous version. That version is downloaded from the server (header
  `X-Maintafox-Rollback-To`, allowed even while a rollout is paused), starts on the backup and
  downloads again from the server what was synchronised after the backup.
- **A release is recalled** while a PC runs it: the server offers the previous release. If no
  migration ran since then, the database is kept as it is. If migrations ran, the pre-update backup
  is restored first, and only when nothing is waiting to be synchronised (otherwise the user is
  asked to synchronise first). If this PC has no backup of that version, it refuses and says to
  contact support.

## Manual reinstall of a previous version (support)
Every release's signed installer is kept as a GitHub release of the desktop repository
(`Maintafox_<version>_x64-setup.exe`). When a PC cannot use the in-app rollback (no internet, no
backup):

1. Download the installer of the wanted version from the GitHub release.
2. If the newer version migrated the database, restore a backup made by the older version
   (Settings > Backup > Restore) before or right after reinstalling.
3. Run the installer (the installer allows going back to an older version).

## What is tested, what is not
- Rust unit tests: journal state machine, downgrade decision, manifest parsing (`src-tauri/src/updater`).
- Vendor API integration test: recall, pause, rollback offer, minimum version, rollout percentage.
- **Not tested end to end:** a real installed build updating itself (needs the owner's updater key
  and a published release). First real check: publish the first release to Internal and update
  one PC.
