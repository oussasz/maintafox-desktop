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
6. **Two GitHub environments (required).** The release workflow has three jobs: it builds the
   installer, signs it, then registers it. The build runs in an environment named `release-build`;
   the signing and the registering run in a protected environment named `release`. Create both once
   (GitHub > the desktop repository > Settings > Environments > New environment):
   - `release`: tick **Required reviewers** and add yourself (and only people you trust to ship to
     every customer). Tick **Prevent self-review** if a second person reviews. Under **Deployment
     branches and tags**, choose "Selected branches and tags" and add the tag rule `v*.*.*`. Put
     the updater key and the publisher token here (Environment secrets), not in the repository:
     `gh secret set TAURI_SIGNING_PRIVATE_KEY --env release`, and the same for
     `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and `MAINTAFOX_RELEASE_PUBLISHER_TOKEN`.
   - `release-build`: the same tag rule `v*.*.*` (reviewers are not needed). Put only the `ES_*`
     code-signing secrets here, if you have them (`gh secret set ES_USERNAME --env release-build`,
     ...). Never put the updater key or the publisher token in `release-build`.
   - Delete the repository-level copies (`gh secret delete TAURI_SIGNING_PRIVATE_KEY`, ...).
   - Settings > Rules > Rulesets: add a tag ruleset for `v*` so only you can create or move release
     tags.
   From then on, each pushed tag waits in GitHub Actions for your "Approve and deploy", twice: once
   for the signing job, once for the registering job.

Check: `gh secret list --env release` shows `TAURI_SIGNING_PRIVATE_KEY`,
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and `MAINTAFOX_RELEASE_PUBLISHER_TOKEN`;
`gh secret list --env release-build` shows only the `ES_*` secrets (or nothing).

How the workflow protects the key: the build job, which installs the npm packages and runs every
build script, never has the updater key. The signing job runs on a fresh computer: it checks out
the repository again, runs no `pnpm install` and nothing from `node_modules`, receives the installer
from the build job (checked by its SHA-256), signs it with the Tauri CLI installed from crates.io at
a pinned version and checksum, and checks the signature against the committed
`src-tauri/updater/maintafox-updater.pub` with node alone (`scripts/release/verify-updater-signature.mjs`):
a secret that does not match the key built into the app stops the release instead of breaking
updates on customers' PCs. The registering job checks both files again before uploading. No build
cache is used, every action is pinned to a commit, and no tag name or variable is ever pasted into a
script.

What remains: the Authenticode (`ES_*`) credentials are in the build step, because Tauri signs the
app and the installer during the build (`signCommand`); a malicious build dependency could read
them (see `docs/release/CODE_SIGNING.md`). The signing job builds the pinned Tauri CLI from
crates.io, so that crate's own dependencies run their build scripts there, before the signing step
and without the key in their environment.

Builds made before step 2 (and developer builds) have no in-app updates; the app says so instead
of pretending to check.

## Shipping a version (Claude does steps 1-3 when you ask for a release)

1. Set the version (for example `1.2.0`) in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`
   and `package.json` (the three must be equal; the workflow checks). Only a plain version
   MAJOR.MINOR.PATCH is released: the release catalog refuses pre-release versions (`0.4.0-dev`,
   `1.2.3-rc.1`), so the workflow stops on them and a pre-release build is never uploaded. The
   installer is then exactly `Maintafox_<version>_x64-setup.exe`, and its updater signature
   (`.sig`, minisign) must name that file in its trusted comment; the workflow checks both, as the
   control API does at upload.
2. Write the release notes in `docs/release/notes/1.2.0.en.md` and `docs/release/notes/1.2.0.fr.md`
   (customers see them before installing).
3. Merge, then tag: `git tag v1.2.0` and `git push origin v1.2.0`. Approve the signing job and then
   the registering job in GitHub Actions (the `release` environment asks you each time). The
   "Release (Windows)" workflow takes about 40-50 minutes: it builds from scratch, without a cache,
   and builds the pinned signer. It stops with a clear message if anything is missing.
4. In the console, the release appears as a draft. Publish it to **Internal** first and install it
   on your own PC (update banner > Install now). Then **Pilot** with a percentage, then **Stable**.

## If something goes wrong

- **Bad release:** Console > Updates > Releases > Recall (with your authenticator code). PCs stop
  being offered it at their next check, and PCs already on it are offered the previous version.
- **Pause everything for a group:** Console > Updates > Rollouts > Pause.
- **Update fails on a PC while updating its data:** the app puts back the backup taken just before
  the update and offers to reinstall the previous version (`docs/release/ROLLBACK.md`).
