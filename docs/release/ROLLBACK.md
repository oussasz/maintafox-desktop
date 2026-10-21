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

## Which offers the app accepts
Checked by the desktop (`updater::accept_offer`), whatever the signature:
- after a failed database update, only the exact version it asked for (`X-Maintafox-Rollback-To`),
  kind `rollback`. A 409 (`rollback_target_not_allowed`), a 204 or any other answer is a refusal:
  nothing else is tried, the refusal is written to `update-journal.json` (`rollback_refusal`), the
  new version keeps showing the recovery screen and never migrates again, and the pre-update backup
  stays staged (it is the database from the next start on). The screen tells the user to keep the
  PC as it is and contact Maintafox support;
- otherwise, an upgrade to a higher version, or the server's rollback off a recalled running
  version (lower, kind `rollback`, reason `recalled_version`).

## Who installs
An administrator of the PC (`adm.settings`, the licence screen's rule) with a fresh identity check.
While the licence requires an update, the licence screen offers it (the app itself is blocked);
the licence check lifts the denial once the installed version meets the minimum.

## Manual reinstall of a previous version (support)
Every release's signed installer is kept as a GitHub release of the desktop repository
(`Maintafox_<version>_x64-setup.exe`). When a PC cannot use the in-app rollback (no internet, no
backup):

1. Download the installer of the wanted version from the GitHub release.
2. If the newer version migrated the database, restore a backup made by the older version
   (Settings > Backup > Restore) before or right after reinstalling.
3. Run the installer (the installer allows going back to an older version).

## What is tested, what is not
- Rust unit tests: journal state machine, downgrade decision, manifest parsing, accepted offers,
  rollback refusals (409, 204, mismatch, upgrade answer), a failed pre-update backup stopping the
  install, and a journal-driven rollback on a real encrypted database (`src-tauri/src/updater`).
- Vendor API integration test: recall, pause, rollback offer, minimum version, rollout percentage.
- **Not tested end to end:** a real installed build updating itself (needs the owner's updater key
  and a published release). First real check: publish the first release to Internal and update
  one PC.
