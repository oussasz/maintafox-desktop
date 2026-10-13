# Windows code signing

Without an Authenticode certificate, Windows SmartScreen shows "Windows protected your PC" when a
customer runs the installer, and some antivirus products are more suspicious. The in-app updater
does not need it (updates are verified with the updater key), but first installs do.

The release workflow never fakes a signature: with no certificate configured it builds, marks the
release `code_signed = false` (shown in the console) and prints a NOT CODE-SIGNED warning.

## Options researched (October 2026)

| Option | Price (approx.) | Usable for an Algerian company? | Automatic signing in GitHub |
|---|---|---|---|
| **SSL.com OV + eSigner cloud signing** | ~US$290/year all in (certificate + eSigner, $180/year for 240 signatures) | To confirm with SSL.com before paying | Yes: CodeSignTool, supported by the workflow |
| Certum OV + SimplySign cloud | ~US$116/year (reseller price) | To confirm (Certum is Polish) | Weak: SimplySign asks for a one-time code from a phone app |
| Azure Artifact Signing (formerly Trusted Signing) | US$9.99/month | **No.** Public trust is only for organisations in the US, Canada, EU, UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway and Israel (Microsoft quickstart, October 2026) | Very good, if you ever have an eligible (e.g. EU) company |
| EV certificate | US$300-700+/year | Stricter checks | Since 2024 it no longer gives instant SmartScreen trust: not worth the extra cost |
| SignPath Foundation, Certum "Open Source" | free / ~€50 | **No**: open-source projects or private individuals only | n/a |

True for all of them: since June 2023 the private key must be in a hardware token or a cloud HSM
(no more certificate files in CI), and since 1 March 2026 a certificate lasts at most about 460
days (renewal every ~15 months). SmartScreen trust still builds up over the first downloads,
whatever the certificate.

**Recommendation: SSL.com OV certificate with eSigner.** It is the only affordable option that
signs unattended in GitHub Actions and that may accept an Algerian company. Fallback: Certum OV
(cheaper, but signing becomes a manual step on your PC). **Not verified:** whether SSL.com or
Certum validate an Algerian company and which documents they accept. Ask them in writing before
paying.

Sources: SSL.com eSigner pricing (ssl.com/guide/esigner-pricing-for-code-signing), SSL.com
CodeSignTool command guide (ssl.com/guide/esigner-codesigntool-command-guide), Microsoft Artifact
Signing quickstart (learn.microsoft.com/azure/artifact-signing/quickstart), SignPath terms
(signpath.org/terms), Certum (certum.eu, shop.certum.eu).

## What the certificate authority usually asks for (OV; confirm with them)

- Company registration: Registre du Commerce extract, NIF, statuts (sworn translation if asked).
- The company's address and a phone number listed in a public directory, or a letter from a
  lawyer or chartered accountant confirming the company, its address and phone number.
- ID of the person ordering, and proof they may act for the company.
- An e-mail address on the company's domain (e.g. @maintafox.systems) helps.

## Turning signing on (after the certificate is issued)

1. In SSL.com, enrol the certificate in eSigner and note the **credential ID** and the **TOTP
   secret** (shown when you set up eSigner's authenticator; keep it secret).
2. Download CodeSignTool for Windows from SSL.com's website and compute its checksum in
   PowerShell: `Get-FileHash <downloaded zip> -Algorithm SHA256`.
3. Repository **variables** (GitHub > Settings > Secrets and variables > Actions > Variables):
   - `MAINTAFOX_CODESIGN_PROVIDER` = `sslcom-esigner`
   - `CODESIGNTOOL_DOWNLOAD_URL` = the address you downloaded the zip from
   - `CODESIGNTOOL_SHA256` = the checksum from step 2
4. Repository **secrets**, typed by you one by one: `gh secret set ES_USERNAME`,
   `gh secret set ES_PASSWORD`, `gh secret set ES_CREDENTIAL_ID`, `gh secret set ES_TOTP_SECRET`.
5. The next release is signed: `tauri build` calls `scripts/release/sign-windows.ps1` for the app,
   the uninstaller and the installer (before the updater signature is computed), and the workflow
   checks that Windows reports the installer's signature as valid. A failure stops the release.

To switch signing off again, delete the variable `MAINTAFOX_CODESIGN_PROVIDER`.
