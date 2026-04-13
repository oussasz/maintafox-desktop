# GitHub Environments Setup

This file describes the manual configuration required in GitHub repository Settings →
Environments for the Maintafox Desktop project.

## staging environment

- **Name:** `staging`
- **Required reviewers:** 1 (designated release manager or senior developer)
- **Deployment branch rule:** only deploy from `staging` branch
- **Environment secrets:**
  - `MAINTAFOX_VPS_URL` — staging VPS API endpoint
  - `MAINTAFOX_VPS_API_TOKEN` — staging API token

## production environment

- **Name:** `production`
- **Required reviewers:** 2 (both must be from the release-manager team)
- **Wait timer:** 5 minutes (gives reviewers time to abort if something looks wrong)
- **Deployment branch rule:** only deploy from `main` branch
- **Environment secrets:**
  - `MAINTAFOX_VPS_URL` — production VPS API endpoint
  - `MAINTAFOX_VPS_API_TOKEN` — production API token
  - `MAINTAFOX_TAURI_SIGNING_KEY` — Tauri bundle signing private key
  - `MAINTAFOX_TAURI_SIGNING_PASSWORD` — Signing key password

## Repository-level secrets (not environment-scoped)

These are available to all workflows regardless of environment:
  - `GITLEAKS_LICENSE` — Gitleaks Pro license (if in use)

## Setting up secrets

1. Go to repository Settings → Secrets and variables → Actions
2. For repository-level secrets: click "New repository secret"
3. For environment secrets: click the environment name, then "Add secret"
Never share secret values over email, chat, or issues.
