# Release workflow guide

For the owner. This guide explains how a new version of Maintafox reaches the PCs of your
customers, which secrets protect that path, and what to do (and never do) when you ship.

It does not repeat the other release documents. Read them for the details:

- One-time setup: [`RELEASE_SETUP.md`](RELEASE_SETUP.md)
- A failed or bad update on a PC: [`ROLLBACK.md`](ROLLBACK.md)
- The code-signing certificate (not set up yet): [`CODE_SIGNING.md`](CODE_SIGNING.md)
- Release notes format: [`notes/README.md`](notes/README.md)

**State of things as of 2026-10-21**

- Version 0.9.0 is tagged on the branch `phase0/security-hardening` (commit `b0c9cb5f`). The first
  release run is building.
- The GitHub environment `release` holds three secrets: `TAURI_SIGNING_PRIVATE_KEY`,
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` and `MAINTAFOX_RELEASE_PUBLISHER_TOKEN`.
- The GitHub environment `release-build` is empty, because there is no code-signing certificate yet.
- The deployment tag rule is `v*.*.*`, and a tag ruleset protects every `v*` tag.
- The updater public key, id `A2B26F6A31E1E8C0`, is committed in
  `src-tauri/updater/maintafox-updater.pub`. It is also the server's `RELEASE_UPDATER_PUBLIC_KEY`.
- The licence server is live, with the release catalog (migration 018), the media download route
  and the Garage object storage (the place where installers are stored).

Words in brackets are explained once, where they first appear.

---

## 1. How it works, in plain words

You do not build or upload anything by hand. You do three things: you create a tag, you approve two
steps in GitHub, and you publish in the vendor console. GitHub and the server do the rest.

```
 YOU                       GITHUB (release.yml)                       LICENCE SERVER
 ---                       --------------------                       --------------
 push a tag v1.0.0  --->   1. BUILD job  (environment release-build)
                              builds the installer
                              has NO updater key, NO token
                                       |
 approve (1st time) <---------------- waits for you
                                       v
                           2. SIGN job   (environment release)
                              fresh computer, no npm packages
                              signs with the updater key
                              checks the signature against the
                              public key committed in the repo
                                       |
 approve (2nd time) <---------------- waits for you
                                       v
                           3. PUBLISH job (environment release)  --->  registers a DRAFT release
                              uses the publisher token                 (offered to nobody)
                              also makes a GitHub release
                              (a stored copy of the installer)

 In the vendor console (Updates > Releases):
 you publish the draft:   Internal  ->  Pilot (with a %)  ->  Stable

 CUSTOMER PCs:
   about every 2 hours, the app asks the licence server "is there an update for me?"
   (it proves who it is with the device activation token)
        |
        v
   a PC administrator confirms with an identity check
        |
        v
   the app locks itself -> backs up the data -> installs -> restarts
        |
        v
   if the database upgrade fails: the app puts the backup back and
   reinstalls the previous version on that backup
```

What each step means:

- **Tag** (a permanent label on one exact point of the code, such as `v1.0.0`). Pushing a tag
  starts the release run. The workflow only accepts tags that look like `vX.Y.Z`.
- **Build job.** It compiles the app and makes the installer, `Maintafox_<version>_x64-setup.exe`.
  It checks that the tag, the three version files and the French and English notes agree. It never
  holds the updater key, so a bad code library cannot steal it.
- **Environment** (a protected folder of secrets and rules in GitHub). `release-build` holds only
  the code-signing secrets, if you have any. `release` holds the updater key and the token, and it
  waits for your "Approve and deploy" before any job that uses it can start.
- **Sign job.** It signs the installer with the **updater key** (the secret that proves an update
  really comes from you). It then checks the signature against the public key stored in the
  repository. If they do not match, the run stops, so a wrong secret never reaches customers.
- **Publish job.** It checks both files again, uploads them to the server with the **publisher
  token** (a password for the automatic upload only), and registers the release as a **draft**
  (hidden, offered to nobody). It also keeps a copy as a GitHub release; support uses it for a
  manual reinstall (see [`ROLLBACK.md`](ROLLBACK.md)).
- **Rings** (groups of PCs that get a release in order). **Internal** is your own test PCs.
  **Pilot** is chosen customers. **Stable** is everyone. A release published to a wider ring also
  reaches the inner rings. You also choose a **rollout percentage** (the share of PCs in the widest
  ring that are offered it). Raising the percentage only adds PCs; it never removes any.
- **The PC side.** The app checks the server about every 2 hours, and also when asked. A PC
  **administrator** must confirm, with a fresh **identity check** (the app asks them to prove who they are again).
  Then the app locks the whole window (a screen covers it, nothing can be saved), backs up the
  database, writes a small journal file, and starts the installer. The app restarts on the new
  version.
- **If the database upgrade fails** on the first start, the new version puts the backup back and
  offers one button to reinstall the previous version. Details: [`ROLLBACK.md`](ROLLBACK.md).

---

## 2. Every key, token and secret

### The table

| What | Where it lives | What it protects | Who holds it |
|---|---|---|---|
| Updater private key (file) | Owner's PC `%USERPROFILE%\.maintafox\updater`; offline USB copy; GitHub environment `release` | Proves an update comes from Maintafox | Owner only |
| Updater key password | Password manager; offline copy; GitHub environment `release` | Opens the key file | Owner only |
| Updater public key (`A2B26F6A31E1E8C0`) | Repository (`src-tauri/updater/maintafox-updater.pub`); built into every installed app; server `.env` `RELEASE_UPDATER_PUBLIC_KEY` | Lets the app and the server check the signature | Public, no secret |
| `MAINTAFOX_RELEASE_PUBLISHER_TOKEN` | GitHub environment `release`; listed (not shown) in the console | Lets the workflow upload drafts to the server | Owner (created in the console) |
| GitHub environments `release`, `release-build` and their reviewers | GitHub > Settings > Environments | Stops a run from using secrets without your approval | Owner |
| Tag ruleset on `v*` and the tag rule `v*.*.*` | GitHub > Settings > Rules and Environments | Stops others from creating, moving or deleting release tags | Owner |
| Device activation tokens | One per PC, stored on the PC; the server keeps a record | Proves which PC is asking the server | The PC; never shown to anyone |
| Vendor licence-signing keys (Ed25519) | Private key: server `.env` only. Public key: pinned in the desktop code | Makes licences and admin recovery codes impossible to forge | Server (owner administers) |
| Authenticode certificate, `ES_*` secrets | Not set up | Removes the Windows SmartScreen warning | Nobody yet |

### Updater private key and its password

- **What it is.** A file (minisign format, a signing system) plus its password. Every update is
  signed with it. The app installs an update only if the signature matches the public key built
  into it.
- **Where it lives.** Three places: your PC (`%USERPROFILE%\.maintafox\updater`), an offline USB
  stick, and the GitHub environment `release` (as `TAURI_SIGNING_PRIVATE_KEY` and
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). The password also lives in your password manager.
  To confirm: that the USB copy and the password-manager entry have actually been made (nothing in
  the code can show this).
- **If it is LOST** (all copies). The installed PCs keep working, but they can never be updated
  again, because they accept only the key they were built with. GitHub still holds a copy as long
  as the `release` environment exists, so you can still ship. But if every copy including GitHub's
  is gone, you must create a new key (see "Replacing the key" below) and reinstall every PC by hand.
  This is why the USB copy matters: GitHub never shows a secret back to you.
- **If it is LEAKED.** This is an emergency. Anyone with the file and its password can sign an
  installer that every PC would accept. A leak alone is not enough to attack: the attacker must also
  get the file onto the server or the storage, or sit between the PC and the server. But you must not
  wait. Do this, in this order:
  1. In the console, pause updates (Updates > Rollouts > Pause) so nothing is offered while you work.
  2. In GitHub, delete the two secrets from the `release` environment. Revoke the publisher token in
     the console (Updates > Releases > Release workflow keys).
  3. Tell Claude: "updater key leaked". Claude and you create a new key (see below), commit the new
     public key, update the server, and ship a new higher version built with it.
  4. Every PC must be reinstalled by hand with that new installer. In-app update cannot do it: the
     old PCs reject anything signed by the new key.
  5. Check the server and the object storage for any release you did not make; recall it.
- **Replacing the key (lost or leaked).** There is no way to change the key inside an installed app.
  This is the only case where you create a second key, and only on purpose, never casually:
  1. Move the old key file and `src-tauri/updater/maintafox-updater.pub` out of the way (keep them).
  2. Run `scripts\release\setup-updater-key.ps1` (see [`RELEASE_SETUP.md`](RELEASE_SETUP.md), step 1).
     The script refuses to run while the old key or the old `.pub` file exists; that is its safety.
  3. Set the new `RELEASE_UPDATER_PUBLIC_KEY` on the server and redeploy (ask Claude: "deploy").
  4. Make a release with a higher version, build it, and install it by hand on each PC. The
     data stays (the installer upgrades the existing install).
  5. Make a new USB copy and a new password-manager entry.
- **How to avoid all this.** Make the USB copy today, keep it offline (never on a shared drive),
  and never email the key.

### Updater public key

- **What it is.** The public half of the pair. It cannot sign; it can only check.
- **Where it lives.** In the repository (`src-tauri/updater/maintafox-updater.pub`, id
  `A2B26F6A31E1E8C0`), compiled into the app, and in the server `.env` as
  `RELEASE_UPDATER_PUBLIC_KEY`.
- **What it protects.** The app checks every update against it. The server checks every uploaded
  installer against it, so a release the apps would reject never reaches the catalog.
- **If LOST.** Nothing is lost: it is in the repository history and in every installed app.
- **If LEAKED.** It is meant to be public. Nothing happens.
- **If changed by mistake.** If the repository key and the server key differ, every upload is
  refused (`updater_signature_invalid`). If the repository key differs from the one the apps were
  built with, the workflow's check stops the run. Never replace it casually (see
  `src-tauri/updater/README.md`).

### `MAINTAFOX_RELEASE_PUBLISHER_TOKEN`

- **What it is.** A password, created in Console > Updates > Releases > Release workflow keys
  (it starts with `mfxrp_`). It is shown once. It lets the publish job upload and register a
  draft. It signs nothing and cannot publish to customers: publishing always needs an operator and
  an authenticator code.
- **Where it lives.** The GitHub environment `release` only. The console lists the key by name.
- **If LOST.** Nothing breaks until the next release. Create a new key in the console, set the
  secret again, then revoke the old one.
- **If LEAKED.** Revoke it at once in the console (it is revocable). Create a new one and set it
  with `gh secret set MAINTAFOX_RELEASE_PUBLISHER_TOKEN --env release`. At worst, someone could
  upload a draft; they cannot sign it with the updater key, so the server would refuse it.
- **To rotate.** Create the new key, set the secret, then revoke the old key (in that order).

### GitHub environments and their reviewers

- **What they are.** `release` (updater key, password, publisher token, required reviewer: you,
  deployment limited to tags `v*.*.*`) and `release-build` (only the `ES_*` code-signing secrets,
  currently empty).
- **What they protect.** Without your approval no job can read the updater key. A secret stored at
  repository level would be readable by any workflow without approval, so none must be stored there.
- **If the environment is deleted.** Its secrets are deleted with it, and GitHub never shows them
  again. You would need the USB copy of the key to restore it. Never delete `release`.
- **If a reviewer account is taken over.** The attacker can approve runs. Keep your GitHub account
  protected with two-step sign-in, and add a second trusted reviewer only if you can trust them
  with shipping to every customer.

### Tag ruleset

- **What it is.** A GitHub rule (Settings > Rules > Rulesets) on tags `v*`, plus the environment's
  deployment rule `v*.*.*`.
- **What it protects.** Only you can create, move or delete release tags. A moved tag would make a
  release point at different code than the one you reviewed.
- **If it is removed or weakened.** Anyone with write access to the repository could push a tag and
  start a run. They still need your approval twice, but you could approve the wrong code by mistake.
  Check the rules page now and then.

### Device activation tokens

- **What they are.** One per PC, created when the PC is activated. The PC uses it to ask the server
  for updates and licence checks. The app never shows it.
- **If LOST or the PC is replaced.** Activate the new PC in the normal way (licence process).
- **If LEAKED** (a PC is stolen or copied). Revoke or suspend that device in the vendor console.
  To confirm: the exact console action and screen for revoking one device (not checked for this guide).

### Vendor licence-signing keys (Ed25519)

- **What they are.** Ed25519 is a signing system. The server signs every licence ("entitlement")
  with a private key. It also signs the one-time **admin recovery codes** (the codes you give a
  plant that lost every administrator). The desktop app has the matching public keys built in
  (`src-tauri/src/entitlements/trusted_keys.rs`; current key id `mfx-ed25519-2026-10`).
- **Where the private key lives.** In the server `.env` only (`ENTITLEMENT_SIGNING_KEY_ID` and
  `ENTITLEMENT_SIGNING_PRIVATE_KEY`). It never leaves the server and is not in any repository.
  To confirm: that the server `.env` (which holds this key) is kept in a safe copy of its own;
  the backup documents cover the database, not this file.
- **If LOST.** The server cannot sign new licences until a new key exists. Existing signed
  licences still verify until they expire. Fix: rotate (below).
- **If LEAKED.** Someone could forge licences and recovery codes. This is an emergency: ask Claude to
  start the licence-key rotation in the console's `key_rotation` runbook, and mark the old key
  compromised (the desktop has a `licensing_trust_keys.is_compromised` flag for this).
- **To rotate (no lock-out).** (1) Generate a new key with
  `api/scripts/generate-entitlement-signing-key.mjs` in the vendor console repository. (2) Ship a
  desktop release that trusts the new public key next to the old one, and wait until PCs have it.
  (3) Only then switch the server to the new key and restart. (4) Keep the old public key trusted
  until the licences it signed have expired. Never switch the server first: PCs would reject every
  new licence.

### Authenticode certificate (optional, not set up)

- **What it is.** A purchased certificate that signs the installer so Windows trusts it. The `ES_*`
  secrets would go in `release-build`.
- **State.** Not set up. See [`CODE_SIGNING.md`](CODE_SIGNING.md) and section 5.
- **If set up and leaked.** Ask the issuer to revoke it at once, and replace it.

---

## 3. How to do a future release

Claude can do steps 1 to 3 when you say "make release 1.0.0". You always do the approvals and the
publishing yourself.

1. **Version and notes pull request (PR).**
   - Set the new version in three files: `package.json`, `src-tauri/Cargo.toml` and
     `src-tauri/tauri.conf.json`. The three must be exactly equal.
   - Use a plain version `MAJOR.MINOR.PATCH` such as `1.0.0`. Versions like `1.0.0-rc.1` or
     `0.4.0-dev` are refused (the run stops).
   - The version must be higher than every version ever published. It can only go up.
   - Add the notes `docs/release/notes/1.0.0.en.md` and `docs/release/notes/1.0.0.fr.md`. Both are
     required and cannot be empty. Write for the users: what changes for them, in a few short lines.
2. **Merge it** into `phase0/security-hardening` (the release branch, see section 4).
3. **Tag.** On `phase0/security-hardening`, after the merge:
   `git tag v1.0.0` then `git push origin v1.0.0`. (Claude does this when you ask.)
4. **Approve twice** in GitHub > Actions > "Release (Windows)": first the sign job, then the publish
   job (button "Approve and deploy"). Before approving, check that the run is yours and that the
   tag commit is the one you intended. The whole run takes about 40 to 50 minutes, because it builds
   from scratch with no saved cache.
5. **Publish in steps.** In Console > Updates > Releases the release shows as a draft.
   - Publish it to **Internal**. Install it on your own test PC (update banner > Install now). Check
     that the app starts, your data is there, and the version number is right.
   - Then **Pilot**, with a rollout percentage. Raise the percentage as you gain confidence.
   - Then **Stable**. Each publish needs your authenticator code.

### Hotfix (urgent fix)

A hotfix is a new release with a higher patch number: if `1.0.0` is bad, ship `1.0.1`, through the
same path (PR, merge, tag, approvals, Internal, Pilot, Stable). Never re-use a version number and
never move or re-create a tag; the catalog is built for versions that only go up (to confirm: the exact refusal message), and a moved tag breaks
the link between the release and its code.

### Forced update

To make an update mandatory, set a minimum version (a "floor") in the console (Updates > Forced
updates). PCs below the floor are told they must update, and the offer ignores the rollout
percentage. **Always publish the release first, then set the floor.** A floor with no published
release for that PC blocks the PC: it is forced to update and nothing is offered. A "required"
policy with no readable minimum blocks every PC. Remove or lower the floor to unblock.

### Bad release

- **Recall** (Console > Updates > Releases > Recall, with your authenticator code). PCs stop being
  offered it. A PC already on it is offered the previous release. If a PC has no previous release
  available, it stays where it is, so publish a fixed release next (a hotfix).
- **Pause** (Console > Updates > Rollouts > Pause). Nothing is offered to that group until you
  resume. Use it to stop, think and decide.

### An update failed on one PC

See [`ROLLBACK.md`](ROLLBACK.md). The app already restores its backup and offers to reinstall the
previous version. The same document has the manual reinstall and the support steps.

---

## 4. Branches

- **Development** happens on `claude/ux-unify-1af493`. Helper sessions open pull requests into it.
- It is merged into **`phase0/security-hardening`**. This is the **release branch**: the version PR
  goes here and release tags are made here. (The 0.9.0 tag is on it.)
- **`main` and `develop` are not used for releases now.**
- `docs/BRANCHING_STRATEGY.md` is **outdated** for this. It describes `develop` as the daily branch,
  `staging` and `main` as the release path, and signed tags made on `main`. `docs/VERSIONING_POLICY.md`
  also says a release workflow tags `main` on merge; the real workflow runs on a pushed tag
  instead. Neither file was changed by this guide. To confirm: whether you want those two documents
  rewritten to match, or the branches renamed later.

---

## 5. What is NOT set up yet

- **Code-signing certificate.** Until it exists, every release says "NOT CODE-SIGNED" in GitHub and
  in the console, and Windows shows a SmartScreen warning ("Windows protected your PC") on first
  installs. In-app updates are not affected. See [`CODE_SIGNING.md`](CODE_SIGNING.md); it also lists
  what to ask the issuer first.
- **Off-site copy of the updater key.** Not shown anywhere as done. Recommended: one offline USB
  copy (kept away from your PC) plus the password in your password manager. Do it before 1.0.
- **Off-site backup of the server.** The server backups are encrypted and kept on the server's own
  disk. They are explained in the vendor console's `deploy/BACKUP_AND_RESTORE.md`. An optional
  nightly copy to a machine on your premises ("on-premise pull", section 8 of that document) exists
  in the code. To confirm: whether that machine is installed and pulling. Until it is, a lost server
  also loses its backups. Also: the backups open only with the **owner's backup key**, which is not
  on the server. Keep it in the password manager and an offline copy; if it is lost, every backup is
  unreadable.
- **Second reviewer.** Only you can approve runs. If you are unavailable, no release can be made.
  That is safe, but think about who could stand in.
- **Automatic updates of the pinned GitHub actions.** Dependabot is configured
  (`.github/dependabot.yml`, weekly, at most 5 pull requests, nothing merged automatically). Review
  those pull requests: each one changes the code that runs with your secrets. To confirm: that
  Dependabot is enabled in the repository settings.
- **A real end-to-end update on an installed PC.** The first release (0.9.0, Internal) is that test.
  [`ROLLBACK.md`](ROLLBACK.md) notes it has not yet been proven on a real installed build.

---

## 6. Never do

- Never create a second updater key (except to replace a lost or leaked one, on purpose, as in
  section 2).
- Never put secrets at repository level, and never put the updater key or the publisher token in
  `release-build`.
- Never move or delete a release tag.
- Never re-use a version, and never publish a version lower than one already published.
- Never publish straight to Stable. Always Internal first, then Pilot.
- Never set a forced-update floor before the release is published.
- Never approve a run you did not start, or whose tag commit you have not checked.
- Never share the key file or its password with anyone, Claude included.
- Never email the `.key` file, or put it in a chat, a shared folder or the repository.
- Never delete the `release` environment.
- Never turn a failing release check into a warning to "get it through". Fix the cause.

---

## Résumé en français

**Comment une version arrive chez les clients.** Vous créez une étiquette (tag) `v1.0.0` sur la
branche `phase0/security-hardening`. GitHub construit l'installateur, puis attend votre accord
(1re approbation) pour le signer avec la clé de mise à jour, puis attend encore votre accord
(2e approbation) pour l'enregistrer sur le serveur de licences en brouillon. Dans la console
fournisseur, vous publiez ensuite : Interne (vos PC de test), Pilote (avec un pourcentage), puis
Stable. Les PC interrogent le serveur environ toutes les 2 heures. Un administrateur du PC
confirme avec une vérification d'identité ; l'application se verrouille, sauvegarde les données,
installe et redémarre. Si la mise à jour de la base échoue, elle remet la sauvegarde et réinstalle
la version précédente.

**Clés et secrets.** La clé privée de mise à jour (fichier + mot de passe) est sur votre PC, sur une
clé USB hors ligne et dans l'environnement GitHub `release`. Si elle est perdue ou volée, les PC
déjà installés n'acceptent que la clé avec laquelle ils ont été construits : il faut créer une
nouvelle clé et réinstaller chaque PC à la main. Une fuite est une urgence : mettre les mises à
jour en pause, supprimer les secrets GitHub, révoquer le jeton, puis remplacer la clé. Le jeton
`MAINTAFOX_RELEASE_PUBLISHER_TOKEN` se crée et se révoque dans la console. Les clés Ed25519 du
fournisseur signent les licences et les codes de récupération administrateur ; la clé privée reste
sur le serveur. Le certificat de signature de code (Authenticode) n'est pas encore en place :
Windows affiche donc un avertissement SmartScreen.

**Prochaine version.** Changer la version dans les trois fichiers (toujours plus haute, version
simple), ajouter les notes EN et FR, fusionner, poser le tag, approuver deux fois, attendre 40 à
50 minutes, puis publier Interne, Pilote, Stable. Correctif urgent : nouvelle version plus haute,
jamais de tag déplacé. Mise à jour forcée : publier d'abord, fixer le minimum ensuite. Mauvaise
version : Rappel (Recall) ou Pause.

**Branches.** Développement sur `claude/ux-unify-1af493`, fusion dans `phase0/security-hardening`
(branche de version). `main` et `develop` ne servent plus aux versions ; `BRANCHING_STRATEGY.md`
est périmé.

**À ne jamais faire.** Créer une seconde clé sans raison, mettre des secrets au niveau du dépôt,
déplacer un tag, réutiliser une version, publier directement en Stable, fixer un minimum avant
publication, approuver une exécution inconnue, partager ou envoyer la clé par courriel, supprimer
l'environnement `release`.
