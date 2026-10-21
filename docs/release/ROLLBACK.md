# Update safety and rollback (desktop, master plan Phase 5.5)

## Before every update
In this order (`updater::run_install`):
0. If a database restore is waiting for the next start (an administrator's, staged in Settings >
   Backup, or one an update could not cancel), no update starts: the banner asks to restart
   Maintafox first. The same check runs again after the download.
1. The app asks the server again. A recall or pause since the last check stops the install, and
   so does any offer other than the one the administrator confirmed (version, kind, release): the
   banner then shows the new offer for a new confirmation. Nothing is switched silently.
2. It downloads the installer while the app stays usable, and checks its updater signature against
   the key built into the app. The signature's trusted comment must name
   `Maintafox_<offered version>_x64-setup.exe`: a correctly signed installer of another version is
   refused. Nothing is written before or during the download.
3. Right after the download the app is locked: a screen covers the whole window (nothing behind
   it can be clicked or reached with the keyboard, nothing to close), every command that needs a
   session is refused, the sync worker pauses, and the window and the tray cannot quit. Then it
   backs up the database (`backups/maintafox-pre-update-<from>-to-<to>-<time>.db`, encrypted like
   the database, recorded in the backup history as `pre_update`, with its checksum) or, for a
   rollback, stages the restore (below), and writes `update-journal.json` next to the database.
4. It starts the installer at once (the app restarts). Work done during the download is in the
   backup. If the installer cannot start, a staged restore is cancelled, the journal says the
   install did not complete, and the app is unlocked again.

The lock leaves one small gap: a save that was already running when the lock started can still
finish during the first moments of the backup (milliseconds). Changes to tables the sync does not
track are never counted by the "waiting to be synchronised" check of a rollback.

**Staged restores are always explained.** Before a rollback stages its restore, the journal records
which restore it is (the pending file and its checksum, `staged_restore`). At the next start,
before the database opens, a staged restore with that checksum is applied only when the journal
justifies it: the rollback was installed (the version that starts is the rollback's), or the new
version could not update the database (`rollback_required`). In every other case (the installer was
cancelled or failed, another version was installed, the update stopped after staging) it is
cancelled and never applied, even when the app could not delete it at the time (a locked file, for
example): the journal still names it, so the next start removes it. A staged restore the journal
does not name, or with another checksum, is one an administrator staged in Settings > Backup and is
applied as usual (staging one in Settings > Backup also clears the journal's record). Only one
install or reinstall runs at a time.

On Windows the updater plugin does not report a declined administrator prompt or an installer that
failed to start: it starts the installer (`ShellExecuteW`), ignores the result and closes the app.
So a cancelled installer shows up only at the next start, where the rule above cancels the staged
restore and the banner says the update did not complete.

## Database migrations are forward-only
An older version cannot open a database that a newer version has migrated. So:

- **The new version cannot update the database** (first start after the update): it checks that
  the pre-update backup is the file that was taken (its recorded checksum is required and must
  match), stages it (the Phase 1.3 restore: integrity and key checked, the half-updated file kept
  as a `pre_restore` backup) and shows "The update could not be completed" with a button that
  reinstalls the previous version. If the backup is missing, has no checksum or changed, nothing is
  restored: the screen says to keep the PC as it is and contact support, offers no reinstall, and
  the database is not migrated again. The recovery reinstall needs no session, but works only on
  the version that failed (the journal's `to_version`) and only for the version it came from. That version is downloaded from the server (header
  `X-Maintafox-Rollback-To`, allowed even while a rollout is paused), starts on the backup and
  downloads again from the server what was synchronised after the backup.
- **A release is recalled** while a PC runs it: the server offers the previous release. If no
  migration ran since then, the database is kept as it is. If migrations ran, the pre-update backup
  is restored first, and only when nothing is waiting to be synchronised (otherwise the user is
  asked to synchronise first). That is checked before the download, again after it, and once more
  after the restore is staged (a change made meanwhile cancels the staging). If this PC has no
  backup of that version or no checksum was recorded for it, it refuses ("the backup is not on this
  computer"); if the backup changed since it was taken (checksum mismatch), it refuses with its own
  message ("the backup has changed since it was taken"). Both say to contact support.

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

## Support: the app stops at start with "the update journal could not be checked"
This start screen means a database restore is staged (`maintafox.db.restore-pending` next to
`maintafox.db` in the app data folder, `%APPDATA%\systems.maintafox.desktop`) and
`update-journal.json` in the same folder cannot be read (damaged or half-written). The app applies
neither on its own: it cannot tell whether the restore is the one an update needs or one left by an
update that did not install.

1. Close Maintafox. Copy the whole app data folder somewhere safe before changing anything.
2. Open `update-journal.json` in Notepad. If it is readable JSON with an obvious defect (cut off at
   the end, for example), compare it with the `UpdateJournal` fields in
   `src-tauri/src/updater/journal.rs` to read its state, and decide with the steps below; do not
   invent values.
3. Decide about the staged restore:
   - the PC was rolling back to an older version and that older version is now installed, or the
     journal said `rollback_required`: the restore is needed. Delete `update-journal.json` and start
     Maintafox: with no journal, the staged restore is applied as an administrator's restore;
   - otherwise (the installer was cancelled, the same version runs as before, or you cannot tell):
     delete `maintafox.db.restore-pending`, then `update-journal.json`, and start Maintafox: the
     database stays as it was, with today's work.
4. When in doubt, keep the copy from step 1 and contact Maintafox support before deleting anything.
   The pre-update backups stay in the backups folder in every case.

## What is tested, what is not
- Rust unit tests: journal state machine, downgrade decision, manifest parsing, accepted offers,
  rollback refusals (409, 204, mismatch, upgrade answer), a failed pre-update backup stopping the
  install, and a journal-driven rollback on a real encrypted database (`src-tauri/src/updater`).
  The install order on real encrypted databases: a change made during the download is in the
  pre-update backup; a rollback with a change made during the download is refused and stages
  nothing; a rollback whose installer cannot start leaves no staged restore; startup cancels a
  restore staged for a rollback that was not installed; the signature must name the offered
  installer; the install is bound to the confirmed offer; unverifiable backups are never restored.
  The journal names a staged restore before it is staged; when the restore cannot be cancelled
  (after a refusal, or after the installer failed to start), the next start cancels it; an
  administrator's restore is never taken for the updater's; no update starts while a restore is
  staged; the write barrier refuses commands from the backup on and comes down when the install
  stops. The lock screen has vitest tests (inert app, focus, keys).
- Release workflow: `scripts/release/verify-updater-signature.mjs` (minisign check against the
  committed key) and the https rule of `publish-release.mjs` have node tests. The workflow itself
  (three jobs: build, sign, publish) has not run on GitHub yet.
- Vendor API integration test: recall, pause, rollback offer, minimum version, rollout percentage.
- **Not tested end to end:** a real installed build updating itself (needs the owner's updater key
  and a published release). First real check: publish the first release to Internal and update
  one PC.
