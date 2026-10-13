# Desktop updater public key

`maintafox-updater.pub` (created by the owner, committed) is the public half of the key pair that
signs every update. `build.rs` embeds it in the app; the app refuses any update not signed by the
matching private key.

- Create it once: `powershell -ExecutionPolicy Bypass -File scripts/release/setup-updater-key.ps1`
  (see `docs/release/RELEASE_SETUP.md`). The private key goes only to your own safe place and to the
  GitHub secret `TAURI_SIGNING_PRIVATE_KEY`; it is never written here.
- Without the file the app builds and runs, but has no in-app updates (Settings says so), and the
  release workflow stops with a clear error.
- Never replace this key casually: installed apps only accept updates signed by the key they were
  built with. A new key means every customer must reinstall by hand once.
