# Release setup (one time) and how to ship a version

Plain-language guide for the owner. Technical background: master plan Phase 5 and
`deploy/RELEASES.md` in the vendor console repository.

## What exists

```
You push a tag  ->  GitHub builds the Windows installer  ->  signs it  ->  uploads it to the server
                ->  the release appears as a DRAFT in Console > Updates > Releases
You (console)   ->  publish to Internal (your test PCs) -> Pilot (chosen customers) -> Stable (everyone),
                    with a rollout percentage; Recall if something goes wrong
Customers' PCs  ->  check every 2 hours, back up their data, download, verify, install, restart
```

## One-time setup (about 20 minutes)

You do every step yourself; nothing secret is ever shared with Claude.

1. **Updater key** (signs every update; the app refuses anything else).
   On your PC, in the `maintafox-desktop` folder, in PowerShell:
   ```
   powershell -ExecutionPolicy Bypass -File scripts\release\setup-updater-key.ps1
   ```
   It asks you for a password (choose a strong one and keep it in your password manager), creates
   the key outside the project folder, writes the public key into
   `src-tauri\updater\maintafox-updater.pub`, and shows you two `gh secret set` commands to type.
   Back up the key file and its password offline: **if they are lost, installed copies of
   Maintafox can never be updated again** (customers would have to reinstall by hand).
2. Ask Claude to commit `src-tauri\updater\maintafox-updater.pub` (it is public).
3. **Server:** the same public key goes in the server's `.env` as `RELEASE_UPDATER_PUBLIC_KEY`
   (the script prints it at the end). Claude does this at the next deploy when you say "deploy".
4. **Release workflow key:** Console > Updates > Releases > Release workflow keys > Create
   ("GitHub Actions"), confirm with your authenticator code, copy the token shown once, then type
   `gh secret set MAINTAFOX_RELEASE_PUBLISHER_TOKEN` and paste the token.
5. **Code signing certificate** (optional at first, strongly recommended before customers):
   see `docs/release/CODE_SIGNING.md`. Until then every release says "NOT CODE-SIGNED" in GitHub
   and in the console, and Windows shows a SmartScreen warning when installing.

Check: `gh secret list` shows `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and
`MAINTAFOX_RELEASE_PUBLISHER_TOKEN`.

Builds made before step 2 (and developer builds) have no in-app updates; the app says so instead
of pretending to check.

## Shipping a version (Claude does steps 1-3 when you ask for a release)

1. Set the version (for example `1.2.0`) in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`
   and `package.json`.
2. Write the release notes in `docs/release/notes/1.2.0.en.md` and `docs/release/notes/1.2.0.fr.md`
   (customers see them before installing).
3. Merge, then tag: `git tag v1.2.0` and `git push origin v1.2.0`. The "Release (Windows)" workflow
   runs (about 20-30 minutes). It stops with a clear message if anything is missing.
4. In the console, the release appears as a draft. Publish it to **Internal** first and install it
   on your own PC (update banner > Install now). Then **Pilot** with a percentage, then **Stable**.

## If something goes wrong

- **Bad release:** Console > Updates > Releases > Recall (with your authenticator code). PCs stop
  being offered it at their next check, and PCs already on it are offered the previous version.
- **Pause everything for a group:** Console > Updates > Rollouts > Pause.
- **Update fails on a PC while updating its data:** the app puts back the backup taken just before
  the update and offers to reinstall the previous version (`docs/release/ROLLBACK.md`).
