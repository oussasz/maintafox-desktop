# Phase 1 · Sub-phase 05 · File 02
# French and English Foundation Pack

## Context and Purpose

File 01 built the namespace registry, the lazy-loading i18next configuration, TypeScript
type safety for `t()`, the Rust locale detection module, and the IPC commands for getting
and setting locale preferences. The architecture is in place; the translation files it
depends on are not yet written.

This file creates the content: every JSON translation file for the six **eager
namespaces** in both French and English. These are the keys that every screen in the
app depends on from the very first render — error messages, validation text, shell
labels, login strings, and formatting tokens. If any key in an eager namespace is
absent, the user sees a key string like `[auth:login.submit]` in the UI immediately.

It also creates the **foundations for the three most-used module namespaces** —
`equipment`, `di` (Intervention Requests), and `ot` (Work Orders) — because Phase 2
starts with those modules and the shape of their key structure must be established now
before any Phase 2 sprint authors invent inconsistent ad-hoc patterns.

## Architecture Rules Applied

- **Key naming convention:** `scope.element.modifier` in camelCase dot-notation.
  - Module: `equipment.list.title`, `di.form.subject.placeholder`
  - Global: `common.action.save`, `errors.auth.badCredentials`
  - Never abbreviate: `auth.form.username.label` not `auth.usr.lbl`
- **All JSON keys must exist in BOTH `fr/` and `en/` files.** Missing a key in
  one locale causes "key fallback" or "[key]" display — caught by the CI parity
  check (built in F04).
- **No inline HTML in translation values.** Use i18next `Trans` component for
  content that needs markup. Keeps JSON files clean and XSS-safe.
- **Interpolation uses double curly braces:** `"Bienvenue, {{name}}"`. Never use
  string concatenation.
- **Count pluralization uses `_one` / `_other` suffixes** per i18next conventions:
  ```json
  "item_one": "1 article",
  "item_other": "{{count}} articles"
  ```
- **`formats.json` contains only tokens**, not display strings. It drives the
  `useFormatter()` hook (built in F03). Value format: ICU-style token string or
  locale-specific configuration object.

## What This File Builds

| Deliverable | Purpose |
|-------------|---------|
| `src/i18n/fr/common.json` | App-wide French labels |
| `src/i18n/en/common.json` | App-wide English labels |
| `src/i18n/fr/auth.json` | Auth French labels |
| `src/i18n/en/auth.json` | Auth English labels |
| `src/i18n/fr/errors.json` | Error message French text |
| `src/i18n/en/errors.json` | Error message English text |
| `src/i18n/fr/validation.json` | Form validation French messages |
| `src/i18n/en/validation.json` | Form validation English messages |
| `src/i18n/fr/formats.json` | French format tokens |
| `src/i18n/en/formats.json` | English format tokens |
| `src/i18n/fr/shell.json` | Shell UI French labels |
| `src/i18n/en/shell.json` | Shell UI English labels |
| `src/i18n/locale-data/fr/equipment.json` | Equipment module French foundation |
| `src/i18n/locale-data/en/equipment.json` | Equipment module English foundation |
| `src/i18n/locale-data/fr/di.json` | Intervention Request French foundation |
| `src/i18n/locale-data/en/di.json` | Intervention Request English foundation |
| `src/i18n/locale-data/fr/ot.json` | Work Order French foundation |
| `src/i18n/locale-data/en/ot.json` | Work Order English foundation |

## Prerequisites

- SP05-F01 complete: `namespaces.ts`, `config.ts`, directory structure in place
- SP04-F03 complete: all permission/role names are in French (already hardcoded in
  Rust seeder) — these do NOT move to JSON yet; only UI strings move here

## Sprint Overview

| Sprint | Name | Agent Deliverable |
|--------|------|-------------------|
| S1 | Eager Namespace Pack — global UI, auth, shell | `common`, `auth`, `errors`, `validation`, `formats`, `shell` for fr + en |
| S2 | Module Namespace Starters — equipment, DI, OT | `equipment`, `di`, `ot` JSON starters for fr + en |
| S3 | Type Generation Verification and Parity Test | Existing `types.ts` augmented with actual key shapes, run parity test |

---

## Sprint S1 — Eager Namespace Pack

### AI Agent Prompt

```
You are a bilingual French/English technical writer and TypeScript engineer.
Your task is to create all 12 JSON files (6 namespaces × 2 languages) for the
eager namespace pack. All strings must be professional French / English.
French is the primary operating language (CMMS context: industrial maintenance).

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/fr/common.json
─────────────────────────────────────────────────────────────────────
```json
{
  "app": {
    "name": "Maintafox",
    "tagline": "Gestion de maintenance industrielle",
    "version": "Version {{version}}",
    "loading": "Chargement...",
    "loadingModule": "Chargement du module..."
  },
  "action": {
    "save":        "Enregistrer",
    "cancel":      "Annuler",
    "confirm":     "Confirmer",
    "delete":      "Supprimer",
    "edit":        "Modifier",
    "add":         "Ajouter",
    "create":      "Créer",
    "close":       "Fermer",
    "back":        "Retour",
    "next":        "Suivant",
    "previous":    "Précédent",
    "search":      "Rechercher",
    "filter":      "Filtrer",
    "reset":       "Réinitialiser",
    "export":      "Exporter",
    "import":      "Importer",
    "refresh":     "Actualiser",
    "view":        "Voir",
    "approve":     "Approuver",
    "reject":      "Rejeter",
    "assign":      "Assigner",
    "unassign":    "Désassigner",
    "activate":    "Activer",
    "deactivate":  "Désactiver",
    "archive":     "Archiver",
    "restore":     "Restaurer",
    "print":       "Imprimer",
    "duplicate":   "Dupliquer",
    "publish":     "Publier",
    "revoke":      "Révoquer",
    "apply":       "Appliquer",
    "select":      "Sélectionner",
    "selectAll":   "Tout sélectionner",
    "clear":       "Effacer",
    "download":    "Télécharger",
    "upload":      "Téléverser",
    "ok":          "OK",
    "yes":         "Oui",
    "no":          "Non",
    "loading":     "Chargement..."
  },
  "status": {
    "active":      "Actif",
    "inactive":    "Inactif",
    "draft":       "Brouillon",
    "pending":     "En attente",
    "inProgress":  "En cours",
    "completed":   "Terminé",
    "cancelled":   "Annulé",
    "closed":      "Clôturé",
    "archived":    "Archivé",
    "deleted":     "Supprimé",
    "approved":    "Approuvé",
    "rejected":    "Rejeté",
    "published":   "Publié",
    "locked":      "Verrouillé",
    "unknown":     "Inconnu"
  },
  "label": {
    "name":          "Nom",
    "description":   "Description",
    "code":          "Code",
    "type":          "Type",
    "category":      "Catégorie",
    "status":        "Statut",
    "date":          "Date",
    "createdAt":     "Créé le",
    "updatedAt":     "Modifié le",
    "createdBy":     "Créé par",
    "updatedBy":     "Modifié par",
    "notes":         "Notes",
    "reference":     "Référence",
    "id":            "Identifiant",
    "yes":           "Oui",
    "no":            "Non",
    "na":            "N/A",
    "required":      "Obligatoire",
    "optional":      "Optionnel",
    "all":           "Tous",
    "none":          "Aucun",
    "total":         "Total",
    "count":         "Nombre",
    "page":          "Page",
    "of":            "sur",
    "resultsPerPage":"Résultats par page",
    "noResults":     "Aucun résultat",
    "noData":        "Aucune donnée disponible",
    "site":          "Site",
    "entity":        "Entité",
    "team":          "Équipe",
    "comments":      "Commentaires",
    "attachments":   "Pièces jointes",
    "history":       "Historique",
    "details":       "Détails",
    "summary":       "Résumé"
  },
  "confirm": {
    "deleteTitle":   "Confirmer la suppression",
    "deleteMessage": "Cette action est irréversible. Voulez-vous vraiment supprimer cet élément ?",
    "unsavedChanges":"Des modifications non enregistrées seront perdues. Continuer ?",
    "dangerousAction":"Cette action est sensible et nécessite une confirmation. Continuer ?"
  },
  "pagination": {
    "showing":       "Affichage de {{from}}–{{to}} sur {{total}}",
    "firstPage":     "Première page",
    "lastPage":      "Dernière page",
    "nextPage":      "Page suivante",
    "previousPage":  "Page précédente"
  },
  "time": {
    "just_now":      "À l'instant",
    "minutes_ago_one":   "Il y a 1 minute",
    "minutes_ago_other": "Il y a {{count}} minutes",
    "hours_ago_one":     "Il y a 1 heure",
    "hours_ago_other":   "Il y a {{count}} heures",
    "days_ago_one":      "Il y a 1 jour",
    "days_ago_other":    "Il y a {{count}} jours"
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/en/common.json
─────────────────────────────────────────────────────────────────────
```json
{
  "app": {
    "name": "Maintafox",
    "tagline": "Industrial Maintenance Management",
    "version": "Version {{version}}",
    "loading": "Loading...",
    "loadingModule": "Loading module..."
  },
  "action": {
    "save":        "Save",
    "cancel":      "Cancel",
    "confirm":     "Confirm",
    "delete":      "Delete",
    "edit":        "Edit",
    "add":         "Add",
    "create":      "Create",
    "close":       "Close",
    "back":        "Back",
    "next":        "Next",
    "previous":    "Previous",
    "search":      "Search",
    "filter":      "Filter",
    "reset":       "Reset",
    "export":      "Export",
    "import":      "Import",
    "refresh":     "Refresh",
    "view":        "View",
    "approve":     "Approve",
    "reject":      "Reject",
    "assign":      "Assign",
    "unassign":    "Unassign",
    "activate":    "Activate",
    "deactivate":  "Deactivate",
    "archive":     "Archive",
    "restore":     "Restore",
    "print":       "Print",
    "duplicate":   "Duplicate",
    "publish":     "Publish",
    "revoke":      "Revoke",
    "apply":       "Apply",
    "select":      "Select",
    "selectAll":   "Select all",
    "clear":       "Clear",
    "download":    "Download",
    "upload":      "Upload",
    "ok":          "OK",
    "yes":         "Yes",
    "no":          "No",
    "loading":     "Loading..."
  },
  "status": {
    "active":      "Active",
    "inactive":    "Inactive",
    "draft":       "Draft",
    "pending":     "Pending",
    "inProgress":  "In progress",
    "completed":   "Completed",
    "cancelled":   "Cancelled",
    "closed":      "Closed",
    "archived":    "Archived",
    "deleted":     "Deleted",
    "approved":    "Approved",
    "rejected":    "Rejected",
    "published":   "Published",
    "locked":      "Locked",
    "unknown":     "Unknown"
  },
  "label": {
    "name":          "Name",
    "description":   "Description",
    "code":          "Code",
    "type":          "Type",
    "category":      "Category",
    "status":        "Status",
    "date":          "Date",
    "createdAt":     "Created at",
    "updatedAt":     "Updated at",
    "createdBy":     "Created by",
    "updatedBy":     "Updated by",
    "notes":         "Notes",
    "reference":     "Reference",
    "id":            "ID",
    "yes":           "Yes",
    "no":            "No",
    "na":            "N/A",
    "required":      "Required",
    "optional":      "Optional",
    "all":           "All",
    "none":          "None",
    "total":         "Total",
    "count":         "Count",
    "page":          "Page",
    "of":            "of",
    "resultsPerPage":"Results per page",
    "noResults":     "No results found",
    "noData":        "No data available",
    "site":          "Site",
    "entity":        "Entity",
    "team":          "Team",
    "comments":      "Comments",
    "attachments":   "Attachments",
    "history":       "History",
    "details":       "Details",
    "summary":       "Summary"
  },
  "confirm": {
    "deleteTitle":   "Confirm deletion",
    "deleteMessage": "This action is irreversible. Are you sure you want to delete this item?",
    "unsavedChanges":"Unsaved changes will be lost. Continue?",
    "dangerousAction":"This is a sensitive action and requires confirmation. Continue?"
  },
  "pagination": {
    "showing":       "Showing {{from}}–{{to}} of {{total}}",
    "firstPage":     "First page",
    "lastPage":      "Last page",
    "nextPage":      "Next page",
    "previousPage":  "Previous page"
  },
  "time": {
    "just_now":      "Just now",
    "minutes_ago_one":   "1 minute ago",
    "minutes_ago_other": "{{count}} minutes ago",
    "hours_ago_one":     "1 hour ago",
    "hours_ago_other":   "{{count}} hours ago",
    "days_ago_one":      "1 day ago",
    "days_ago_other":    "{{count}} days ago"
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/fr/auth.json
─────────────────────────────────────────────────────────────────────
```json
{
  "login": {
    "title":           "Connexion",
    "subtitle":        "Connectez-vous pour accéder à Maintafox",
    "form": {
      "username": {
        "label":       "Identifiant",
        "placeholder": "Votre identifiant"
      },
      "password": {
        "label":       "Mot de passe",
        "placeholder": "Votre mot de passe"
      },
      "submit":        "Se connecter",
      "submitting":    "Connexion en cours..."
    },
    "error": {
      "badCredentials": "Identifiant ou mot de passe invalide.",
      "accountLocked":  "Compte temporairement verrouillé. Réessayez dans quelques minutes.",
      "networkRequired":"La première connexion sur cet appareil nécessite une connexion réseau.",
      "deviceRevoked":  "Cet appareil a été révoqué. Connexion en ligne requise.",
      "offlineExpired": "Fenêtre de connexion hors ligne expirée. Connexion en ligne requise.",
      "unknown":        "Une erreur inattendue s'est produite. Veuillez réessayer."
    }
  },
  "logout": {
    "label":            "Déconnexion",
    "confirm":          "Voulez-vous vous déconnecter ?",
    "success":          "Vous êtes déconnecté."
  },
  "session": {
    "expired": {
      "title":          "Session expirée",
      "message":        "Votre session a expiré. Veuillez vous reconnecter.",
      "action":         "Se reconnecter"
    },
    "idleLocked": {
      "title":          "Session verrouillée",
      "message":        "Votre session est verrouillée en raison d'inactivité.",
      "unlockPrompt":   "Entrez votre mot de passe pour déverrouiller",
      "unlockAction":   "Déverrouiller",
      "unlocking":      "Vérification..."
    },
    "forcePasswordChange": {
      "title":          "Changement de mot de passe requis",
      "message":        "Vous devez définir un nouveau mot de passe avant de continuer.",
      "newPassword":    "Nouveau mot de passe",
      "confirmPassword":"Confirmer le mot de passe",
      "submit":         "Changer le mot de passe",
      "success":        "Mot de passe modifié avec succès."
    }
  },
  "stepUp": {
    "title":            "Confirmation d'identité requise",
    "message":          "Cette action est sensible. Saisissez votre mot de passe pour confirmer.",
    "passwordLabel":    "Mot de passe",
    "submit":           "Confirmer l'identité",
    "submitting":       "Vérification...",
    "success":          "Identité confirmée.",
    "error":            "Mot de passe incorrect. Veuillez réessayer."
  },
  "device": {
    "trusted":          "Appareil de confiance",
    "notTrusted":       "Appareil non enregistré",
    "offlineAllowed":   "Connexion hors ligne autorisée ({{hours}}h restantes)",
    "offlineExpired":   "Accès hors ligne expiré",
    "revokeTitle":      "Révoquer cet appareil",
    "revokeConfirm":    "L'accès hors ligne de cet appareil sera supprimé. Continuer ?",
    "revokeSuccess":    "Appareil révoqué avec succès."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/en/auth.json
─────────────────────────────────────────────────────────────────────
```json
{
  "login": {
    "title":           "Sign in",
    "subtitle":        "Sign in to access Maintafox",
    "form": {
      "username": {
        "label":       "Username",
        "placeholder": "Your username"
      },
      "password": {
        "label":       "Password",
        "placeholder": "Your password"
      },
      "submit":        "Sign in",
      "submitting":    "Signing in..."
    },
    "error": {
      "badCredentials": "Invalid username or password.",
      "accountLocked":  "Account temporarily locked. Please try again in a few minutes.",
      "networkRequired":"First login on this device requires a network connection.",
      "deviceRevoked":  "This device has been revoked. Online login required.",
      "offlineExpired": "Offline login window expired. Online login required.",
      "unknown":        "An unexpected error occurred. Please try again."
    }
  },
  "logout": {
    "label":            "Sign out",
    "confirm":          "Do you want to sign out?",
    "success":          "You have been signed out."
  },
  "session": {
    "expired": {
      "title":          "Session expired",
      "message":        "Your session has expired. Please sign in again.",
      "action":         "Sign in again"
    },
    "idleLocked": {
      "title":          "Session locked",
      "message":        "Your session is locked due to inactivity.",
      "unlockPrompt":   "Enter your password to unlock",
      "unlockAction":   "Unlock",
      "unlocking":      "Verifying..."
    },
    "forcePasswordChange": {
      "title":          "Password change required",
      "message":        "You must set a new password before continuing.",
      "newPassword":    "New password",
      "confirmPassword":"Confirm password",
      "submit":         "Change password",
      "success":        "Password changed successfully."
    }
  },
  "stepUp": {
    "title":            "Identity confirmation required",
    "message":          "This is a sensitive action. Enter your password to confirm.",
    "passwordLabel":    "Password",
    "submit":           "Confirm identity",
    "submitting":       "Verifying...",
    "success":          "Identity confirmed.",
    "error":            "Incorrect password. Please try again."
  },
  "device": {
    "trusted":          "Trusted device",
    "notTrusted":       "Unregistered device",
    "offlineAllowed":   "Offline login allowed ({{hours}}h remaining)",
    "offlineExpired":   "Offline access expired",
    "revokeTitle":      "Revoke this device",
    "revokeConfirm":    "Offline access for this device will be removed. Continue?",
    "revokeSuccess":    "Device revoked successfully."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/fr/errors.json
─────────────────────────────────────────────────────────────────────
```json
{
  "generic": {
    "unexpected":       "Une erreur inattendue s'est produite.",
    "retry":            "Veuillez réessayer.",
    "contactSupport":   "Si le problème persiste, contactez le support.",
    "notFound":         "L'élément demandé est introuvable.",
    "forbidden":        "Vous n'êtes pas autorisé à effectuer cette action.",
    "serverError":      "Erreur interne du serveur.",
    "timeout":          "La requête a expiré. Vérifiez votre connexion.",
    "offline":          "Impossible de contacter le serveur. Vérifiez votre connexion réseau."
  },
  "auth": {
    "notAuthenticated": "Vous devez être connecté pour effectuer cette action.",
    "sessionExpired":   "Votre session a expiré. Reconnectez-vous.",
    "permissionDenied": "Permission refusée.",
    "stepUpRequired":   "Cette action nécessite une re-confirmation d'identité.",
    "badCredentials":   "Identifiant ou mot de passe invalide.",
    "accountLocked":    "Compte verrouillé. Réessayez dans {{minutes}} minutes."
  },
  "database": {
    "generic":          "Erreur de base de données.",
    "constraint":       "Violation de contrainte : cette valeur existe déjà.",
    "notFound":         "Enregistrement introuvable : {{entity}} #{{id}}."
  },
  "validation": {
    "generic":          "Données invalides. Vérifiez les champs du formulaire.",
    "unsupportedLocale":"Langue non supportée : {{locale}}."
  },
  "device": {
    "keyringUnavailable": "Le trousseau de clés système est inaccessible.",
    "fingerprintFailed":  "Impossible de calculer l'empreinte de cet appareil."
  },
  "io": {
    "generic":          "Erreur d'entrée/sortie : {{message}}"
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/en/errors.json
─────────────────────────────────────────────────────────────────────
```json
{
  "generic": {
    "unexpected":       "An unexpected error occurred.",
    "retry":            "Please try again.",
    "contactSupport":   "If the problem persists, contact support.",
    "notFound":         "The requested item was not found.",
    "forbidden":        "You are not authorized to perform this action.",
    "serverError":      "Internal server error.",
    "timeout":          "The request timed out. Check your connection.",
    "offline":          "Unable to reach the server. Check your network connection."
  },
  "auth": {
    "notAuthenticated": "You must be signed in to perform this action.",
    "sessionExpired":   "Your session has expired. Please sign in again.",
    "permissionDenied": "Permission denied.",
    "stepUpRequired":   "This action requires identity re-confirmation.",
    "badCredentials":   "Invalid username or password.",
    "accountLocked":    "Account locked. Try again in {{minutes}} minutes."
  },
  "database": {
    "generic":          "Database error.",
    "constraint":       "Constraint violation: this value already exists.",
    "notFound":         "Record not found: {{entity}} #{{id}}."
  },
  "validation": {
    "generic":          "Invalid data. Check the form fields.",
    "unsupportedLocale":"Unsupported language: {{locale}}."
  },
  "device": {
    "keyringUnavailable": "The system keyring is not accessible.",
    "fingerprintFailed":  "Unable to compute this device's fingerprint."
  },
  "io": {
    "generic":          "I/O error: {{message}}"
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/fr/validation.json
─────────────────────────────────────────────────────────────────────
```json
{
  "required":             "Ce champ est obligatoire.",
  "minLength":            "Minimum {{min}} caractères requis.",
  "maxLength":            "Maximum {{max}} caractères autorisés.",
  "minValue":             "La valeur minimale est {{min}}.",
  "maxValue":             "La valeur maximale est {{max}}.",
  "pattern": {
    "generic":            "Format invalide.",
    "email":              "Adresse e-mail invalide.",
    "phone":              "Numéro de téléphone invalide.",
    "url":                "URL invalide.",
    "alphanumeric":       "Lettres et chiffres uniquement.",
    "code":               "Code invalide (lettres majuscules, chiffres, tirets et underscores uniquement).",
    "permissionName":     "Format de permission invalide (ex.: eq.view).",
    "date":               "Date invalide. Format attendu : JJ/MM/AAAA.",
    "positiveNumber":     "La valeur doit être un nombre positif."
  },
  "unique":               "Cette valeur existe déjà.",
  "password": {
    "tooShort":           "Le mot de passe doit comporter au moins 10 caractères.",
    "tooWeak":            "Le mot de passe doit contenir majuscules, minuscules, chiffres et caractères spéciaux.",
    "mismatch":           "Les mots de passe ne correspondent pas.",
    "sameAsCurrent":      "Le nouveau mot de passe doit être différent de l'actuel.",
    "compromised":        "Ce mot de passe est trop courant. Choisissez-en un autre."
  },
  "date": {
    "invalid":            "Date invalide.",
    "beforeMin":          "La date ne peut pas être antérieure au {{min}}.",
    "afterMax":           "La date ne peut pas être postérieure au {{max}}.",
    "endBeforeStart":     "La date de fin doit être postérieure à la date de début."
  },
  "file": {
    "tooLarge":           "Fichier trop volumineux. Taille maximale : {{maxSizeMb}} Mo.",
    "invalidType":        "Type de fichier non autorisé. Types acceptés : {{types}}.",
    "required":           "Veuillez sélectionner un fichier."
  },
  "form": {
    "hasErrors":          "Le formulaire contient des erreurs. Veuillez les corriger avant de soumettre.",
    "saved":              "Enregistrement réussi.",
    "saveError":          "Échec de l'enregistrement. Veuillez réessayer."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/en/validation.json
─────────────────────────────────────────────────────────────────────
```json
{
  "required":             "This field is required.",
  "minLength":            "Minimum {{min}} characters required.",
  "maxLength":            "Maximum {{max}} characters allowed.",
  "minValue":             "Minimum value is {{min}}.",
  "maxValue":             "Maximum value is {{max}}.",
  "pattern": {
    "generic":            "Invalid format.",
    "email":              "Invalid email address.",
    "phone":              "Invalid phone number.",
    "url":                "Invalid URL.",
    "alphanumeric":       "Letters and numbers only.",
    "code":               "Invalid code (uppercase letters, numbers, hyphens and underscores only).",
    "permissionName":     "Invalid permission format (e.g., eq.view).",
    "date":               "Invalid date. Expected format: MM/DD/YYYY.",
    "positiveNumber":     "Value must be a positive number."
  },
  "unique":               "This value already exists.",
  "password": {
    "tooShort":           "Password must be at least 10 characters.",
    "tooWeak":            "Password must include uppercase, lowercase, numbers and special characters.",
    "mismatch":           "Passwords do not match.",
    "sameAsCurrent":      "New password must differ from the current one.",
    "compromised":        "This password is too common. Please choose another."
  },
  "date": {
    "invalid":            "Invalid date.",
    "beforeMin":          "Date cannot be before {{min}}.",
    "afterMax":           "Date cannot be after {{max}}.",
    "endBeforeStart":     "End date must be after start date."
  },
  "file": {
    "tooLarge":           "File too large. Maximum size: {{maxSizeMb}} MB.",
    "invalidType":        "File type not allowed. Accepted types: {{types}}.",
    "required":           "Please select a file."
  },
  "form": {
    "hasErrors":          "The form contains errors. Please correct them before submitting.",
    "saved":              "Saved successfully.",
    "saveError":          "Save failed. Please try again."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/fr/formats.json
─────────────────────────────────────────────────────────────────────
Note: These are tokens consumed by the useFormatter() hook (built in F03).
Values are NOT user-visible strings — they are locale-specific tokens.
```json
{
  "date": {
    "short":       "dd/MM/yyyy",
    "medium":      "d MMM yyyy",
    "long":        "d MMMM yyyy",
    "full":        "EEEE d MMMM yyyy",
    "monthYear":   "MMMM yyyy",
    "timeShort":   "HH:mm",
    "timeMedium":  "HH:mm:ss",
    "dateTime":    "dd/MM/yyyy HH:mm",
    "dateTimeFull":"d MMMM yyyy à HH:mm"
  },
  "number": {
    "decimal":         ",",
    "thousands":       "\u00a0",
    "precision":       2,
    "currencySymbol":  "€",
    "currencyPosition":"after",
    "percentSymbol":   "%",
    "locale":          "fr-FR"
  },
  "currency": {
    "default":    "EUR",
    "pattern":    "{{amount}}\u00a0{{symbol}}"
  },
  "weekStartsOn": 1,
  "firstDayOfYear": 4
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/en/formats.json
─────────────────────────────────────────────────────────────────────
```json
{
  "date": {
    "short":       "MM/dd/yyyy",
    "medium":      "MMM d, yyyy",
    "long":        "MMMM d, yyyy",
    "full":        "EEEE, MMMM d, yyyy",
    "monthYear":   "MMMM yyyy",
    "timeShort":   "h:mm a",
    "timeMedium":  "h:mm:ss a",
    "dateTime":    "MM/dd/yyyy h:mm a",
    "dateTimeFull":"MMMM d, yyyy at h:mm a"
  },
  "number": {
    "decimal":         ".",
    "thousands":       ",",
    "precision":       2,
    "currencySymbol":  "$",
    "currencyPosition":"before",
    "percentSymbol":   "%",
    "locale":          "en-US"
  },
  "currency": {
    "default":    "USD",
    "pattern":    "{{symbol}}{{amount}}"
  },
  "weekStartsOn": 0,
  "firstDayOfYear": 1
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/fr/shell.json
─────────────────────────────────────────────────────────────────────
```json
{
  "topBar": {
    "notifications":   "Notifications",
    "noNotifications": "Aucune notification",
    "userMenu":        "Menu utilisateur",
    "profile":         "Mon profil",
    "settings":        "Paramètres",
    "language":        "Langue",
    "logout":          "Déconnexion",
    "help":            "Aide",
    "about":           "À propos"
  },
  "sidebar": {
    "expand":          "Développer le menu",
    "collapse":        "Réduire le menu",
    "nav": {
      "dashboard":     "Tableau de bord",
      "equipment":     "Équipements",
      "di":            "Demandes d'intervention",
      "workOrders":    "Ordres de travail",
      "planning":      "Planification",
      "pm":            "Maintenance préventive",
      "inventory":     "Inventaire",
      "personnel":     "Personnel",
      "reports":       "Rapports",
      "admin":         "Administration",
      "settings":      "Paramètres",
      "help":          "Aide & Documentation"
    }
  },
  "statusBar": {
    "online":          "En ligne",
    "offline":         "Hors ligne",
    "syncing":         "Synchronisation...",
    "syncError":       "Erreur de synchronisation",
    "lastSync":        "Dernière sync. : {{time}}",
    "appVersion":      "Maintafox {{version}}"
  },
  "tray": {
    "show":            "Afficher Maintafox",
    "hide":            "Masquer",
    "quit":            "Quitter"
  },
  "session": {
    "timeRemaining":   "Session expire dans {{minutes}} min"
  },
  "search": {
    "placeholder":     "Recherche globale...",
    "noResults":       "Aucun résultat pour «\u00a0{{query}}\u00a0»",
    "hint":            "Appuyez sur Entrée pour rechercher"
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/en/shell.json
─────────────────────────────────────────────────────────────────────
```json
{
  "topBar": {
    "notifications":   "Notifications",
    "noNotifications": "No notifications",
    "userMenu":        "User menu",
    "profile":         "My profile",
    "settings":        "Settings",
    "language":        "Language",
    "logout":          "Sign out",
    "help":            "Help",
    "about":           "About"
  },
  "sidebar": {
    "expand":          "Expand menu",
    "collapse":        "Collapse menu",
    "nav": {
      "dashboard":     "Dashboard",
      "equipment":     "Equipment",
      "di":            "Intervention Requests",
      "workOrders":    "Work Orders",
      "planning":      "Planning",
      "pm":            "Preventive Maintenance",
      "inventory":     "Inventory",
      "personnel":     "Personnel",
      "reports":       "Reports",
      "admin":         "Administration",
      "settings":      "Settings",
      "help":          "Help & Documentation"
    }
  },
  "statusBar": {
    "online":          "Online",
    "offline":         "Offline",
    "syncing":         "Syncing...",
    "syncError":       "Sync error",
    "lastSync":        "Last sync: {{time}}",
    "appVersion":      "Maintafox {{version}}"
  },
  "tray": {
    "show":            "Show Maintafox",
    "hide":            "Hide",
    "quit":            "Quit"
  },
  "session": {
    "timeRemaining":   "Session expires in {{minutes}} min"
  },
  "search": {
    "placeholder":     "Global search...",
    "noResults":       "No results for \"{{query}}\"",
    "hint":            "Press Enter to search"
  }
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- All 12 JSON files exist and are valid JSON (no trailing commas, no comments)
- `pnpm run dev`: launch the app in French — shell labels are all in French
- `pnpm run dev`: switch language to English via DevTools — shell labels switch
- No "[ns:key]" fallback strings visible on the login screen or shell
- `pnpm run typecheck` passes (the JSON shapes must match what types.ts augmented)
```

---

### Supervisor Verification — Sprint S1

**V1 — All 12 JSON files are valid JSON.**
Run: `Get-ChildItem src/i18n/fr, src/i18n/en -Filter "*.json" | ForEach-Object { node -e "JSON.parse(require('fs').readFileSync('$($_.FullName)'))" ; Write-Host "$($_.Name): OK" }`
Every file must print `OK`. If any throws a SyntaxError, the file has invalid JSON.
Fix the error (usually a trailing comma) before proceeding.

**V2 — Shell labels display in French.**
Launch `pnpm run dev`. Without logging in, the system tray must show French labels.
Open DevTools → Elements and inspect the sidebar or top bar. Labels should come from
the `shell.json` file (e.g., sidebar navigation label should be "Équipements", not
"Equipment"). If English labels appear, the `fr` locale is not being set as default.

**V3 — No fallback key strings visible on login screen.**
On the login screen, check that all visible text is proper French (or English if switched).
None of the text should show `[auth:login.title]` or similar patterns. If patterns appear,
a key in `auth.json` is missing or misspelled. Check the DevTools console for i18next
warning messages to find which key is missing.

---

## Sprint S2 — Module Namespace Starters

### AI Agent Prompt

```
You are a bilingual French/English CMMS domain expert and TypeScript engineer.
Your task is to create the translation file starters for the three highest-priority
module namespaces: equipment (§6.3), di — Intervention Requests (§6.4), and ot —
Work Orders (§6.5). These files define the key structure that all Phase 2 sprints
MUST follow. The key structure must be consistent across all modules.

Key structure convention (apply to all three modules):
  {module}.page.title          → Module page title
  {module}.list.*              → List view labels
  {module}.detail.*            → Detail / read view labels
  {module}.form.*              → Create/edit form labels
  {module}.status.*            → Status codes to translated labels
  {module}.action.*            → Module-specific action labels (beyond common)
  {module}.empty.*             → Empty state messages

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/locale-data/fr/equipment.json
─────────────────────────────────────────────────────────────────────
```json
{
  "page": {
    "title":         "Équipements",
    "titleDetail":   "Détail de l'équipement",
    "titleNew":      "Nouvel équipement",
    "titleEdit":     "Modifier l'équipement"
  },
  "list": {
    "columns": {
      "code":        "Code",
      "name":        "Désignation",
      "class":       "Classe",
      "site":        "Site",
      "status":      "Statut",
      "criticality": "Criticité",
      "family":      "Famille",
      "parentName":  "Équipement parent"
    },
    "filters": {
      "status":      "Statut",
      "class":       "Classe",
      "site":        "Site",
      "criticality": "Criticité"
    }
  },
  "detail": {
    "sections": {
      "identity":    "Identité",
      "location":    "Localisation",
      "technical":   "Données techniques",
      "lifecycle":   "Cycle de vie",
      "meters":      "Compteurs",
      "documents":   "Documents",
      "history":     "Historique"
    },
    "fields": {
      "code":        "Code équipement",
      "name":        "Désignation",
      "class":       "Classe",
      "family":      "Famille",
      "parentEquipment": "Équipement parent",
      "criticality": "Criticité",
      "status":      "Statut",
      "site":        "Site",
      "entity":      "Entité",
      "manufacturer": "Fabricant",
      "model":       "Modèle",
      "serialNumber": "N° de série",
      "commissioningDate": "Date de mise en service",
      "endOfLifeDate": "Date de fin de vie",
      "technicalNotes": "Notes techniques"
    }
  },
  "form": {
    "identity": {
      "title":       "Identité de l'équipement",
      "code": {
        "label":     "Code équipement",
        "placeholder":"EQ-001",
        "hint":      "Code unique identifiant l'équipement."
      },
      "name": {
        "label":     "Désignation",
        "placeholder":"Nom descriptif de l'équipement"
      }
    },
    "classification": {
      "title":       "Classification"
    }
  },
  "status": {
    "operational":   "En service",
    "maintenance":   "En maintenance",
    "decommissioned":"Déclassé",
    "standby":       "En veille",
    "scrapped":      "Mis au rebut"
  },
  "action": {
    "addMeter":      "Ajouter un compteur",
    "attachDocument":"Joindre un document",
    "viewHistory":   "Voir l'historique",
    "reportFault":   "Signaler une panne",
    "createDI":      "Créer une demande d'intervention"
  },
  "empty": {
    "list":          "Aucun équipement trouvé.",
    "listHint":      "Créez votre premier équipement ou ajustez les filtres.",
    "noMeters":      "Aucun compteur enregistré.",
    "noDocuments":   "Aucun document joint."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/locale-data/en/equipment.json
─────────────────────────────────────────────────────────────────────
```json
{
  "page": {
    "title":         "Equipment",
    "titleDetail":   "Equipment detail",
    "titleNew":      "New equipment",
    "titleEdit":     "Edit equipment"
  },
  "list": {
    "columns": {
      "code":        "Code",
      "name":        "Name",
      "class":       "Class",
      "site":        "Site",
      "status":      "Status",
      "criticality": "Criticality",
      "family":      "Family",
      "parentName":  "Parent equipment"
    },
    "filters": {
      "status":      "Status",
      "class":       "Class",
      "site":        "Site",
      "criticality": "Criticality"
    }
  },
  "detail": {
    "sections": {
      "identity":    "Identity",
      "location":    "Location",
      "technical":   "Technical data",
      "lifecycle":   "Lifecycle",
      "meters":      "Meters",
      "documents":   "Documents",
      "history":     "History"
    },
    "fields": {
      "code":        "Equipment code",
      "name":        "Name",
      "class":       "Class",
      "family":      "Family",
      "parentEquipment": "Parent equipment",
      "criticality": "Criticality",
      "status":      "Status",
      "site":        "Site",
      "entity":      "Entity",
      "manufacturer": "Manufacturer",
      "model":       "Model",
      "serialNumber": "Serial number",
      "commissioningDate": "Commissioning date",
      "endOfLifeDate": "End-of-life date",
      "technicalNotes": "Technical notes"
    }
  },
  "form": {
    "identity": {
      "title":       "Equipment identity",
      "code": {
        "label":     "Equipment code",
        "placeholder":"EQ-001",
        "hint":      "Unique code identifying this equipment."
      },
      "name": {
        "label":     "Name",
        "placeholder":"Descriptive equipment name"
      }
    },
    "classification": {
      "title":       "Classification"
    }
  },
  "status": {
    "operational":   "Operational",
    "maintenance":   "Under maintenance",
    "decommissioned":"Decommissioned",
    "standby":       "Standby",
    "scrapped":      "Scrapped"
  },
  "action": {
    "addMeter":      "Add meter",
    "attachDocument":"Attach document",
    "viewHistory":   "View history",
    "reportFault":   "Report fault",
    "createDI":      "Create intervention request"
  },
  "empty": {
    "list":          "No equipment found.",
    "listHint":      "Create your first equipment record or adjust the filters.",
    "noMeters":      "No meters recorded.",
    "noDocuments":   "No documents attached."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/locale-data/fr/di.json (Demandes d'Intervention)
─────────────────────────────────────────────────────────────────────
```json
{
  "page": {
    "title":         "Demandes d'intervention",
    "titleDetail":   "Détail de la demande",
    "titleNew":      "Nouvelle demande",
    "titleEdit":     "Modifier la demande"
  },
  "list": {
    "columns": {
      "number":      "N°",
      "subject":     "Objet",
      "equipment":   "Équipement",
      "status":      "Statut",
      "priority":    "Priorité",
      "reportedBy":  "Déclarant",
      "reportedAt":  "Date déclaration",
      "assignedTo":  "Assigné à",
      "sla":         "SLA"
    },
    "filters": {
      "status":      "Statut",
      "priority":    "Priorité",
      "assignedTo":  "Assigné à",
      "dateRange":   "Période"
    }
  },
  "detail": {
    "sections": {
      "request":     "Demande",
      "analysis":    "Analyse",
      "resolution":  "Résolution",
      "history":     "Historique"
    },
    "fields": {
      "number":      "Numéro",
      "subject":     "Objet",
      "description": "Description",
      "equipment":   "Équipement concerné",
      "priority":    "Priorité",
      "status":      "Statut",
      "reportedBy":  "Déclaré par",
      "reportedAt":  "Date de déclaration",
      "assignedTo":  "Assigné à",
      "dueDate":     "Échéance",
      "closedAt":    "Date de clôture",
      "conclusion":  "Conclusion"
    }
  },
  "form": {
    "subject": {
      "label":       "Objet",
      "placeholder": "Décrivez brièvement le problème"
    },
    "description": {
      "label":       "Description détaillée",
      "placeholder": "Décrivez le symptôme, les conditions d'apparition..."
    },
    "equipment": {
      "label":       "Équipement concerné",
      "placeholder": "Sélectionnez un équipement"
    },
    "priority": {
      "label":       "Priorité"
    }
  },
  "status": {
    "new":           "Nouvelle",
    "inReview":      "En examen",
    "approved":      "Approuvée",
    "rejected":      "Rejetée",
    "inProgress":    "En cours",
    "resolved":      "Résolue",
    "closed":        "Clôturée",
    "cancelled":     "Annulée"
  },
  "priority": {
    "low":           "Basse",
    "medium":        "Normale",
    "high":          "Haute",
    "critical":      "Critique"
  },
  "action": {
    "approve":       "Approuver",
    "reject":        "Rejeter",
    "assignTo":      "Assigner à",
    "convertToWO":   "Convertir en OT",
    "close":         "Clôturer",
    "addComment":    "Ajouter un commentaire"
  },
  "empty": {
    "list":          "Aucune demande d'intervention.",
    "listHint":      "Créez une nouvelle demande ou ajustez les filtres.",
    "noComments":    "Aucun commentaire pour cette demande."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/locale-data/en/di.json
─────────────────────────────────────────────────────────────────────
```json
{
  "page": {
    "title":         "Intervention Requests",
    "titleDetail":   "Request detail",
    "titleNew":      "New request",
    "titleEdit":     "Edit request"
  },
  "list": {
    "columns": {
      "number":      "No.",
      "subject":     "Subject",
      "equipment":   "Equipment",
      "status":      "Status",
      "priority":    "Priority",
      "reportedBy":  "Reported by",
      "reportedAt":  "Reported on",
      "assignedTo":  "Assigned to",
      "sla":         "SLA"
    },
    "filters": {
      "status":      "Status",
      "priority":    "Priority",
      "assignedTo":  "Assigned to",
      "dateRange":   "Date range"
    }
  },
  "detail": {
    "sections": {
      "request":     "Request",
      "analysis":    "Analysis",
      "resolution":  "Resolution",
      "history":     "History"
    },
    "fields": {
      "number":      "Number",
      "subject":     "Subject",
      "description": "Description",
      "equipment":   "Equipment",
      "priority":    "Priority",
      "status":      "Status",
      "reportedBy":  "Reported by",
      "reportedAt":  "Reported on",
      "assignedTo":  "Assigned to",
      "dueDate":     "Due date",
      "closedAt":    "Closed on",
      "conclusion":  "Conclusion"
    }
  },
  "form": {
    "subject": {
      "label":       "Subject",
      "placeholder": "Briefly describe the problem"
    },
    "description": {
      "label":       "Detailed description",
      "placeholder": "Describe the symptom, conditions..."
    },
    "equipment": {
      "label":       "Equipment",
      "placeholder": "Select equipment"
    },
    "priority": {
      "label":       "Priority"
    }
  },
  "status": {
    "new":           "New",
    "inReview":      "In review",
    "approved":      "Approved",
    "rejected":      "Rejected",
    "inProgress":    "In progress",
    "resolved":      "Resolved",
    "closed":        "Closed",
    "cancelled":     "Cancelled"
  },
  "priority": {
    "low":           "Low",
    "medium":        "Medium",
    "high":          "High",
    "critical":      "Critical"
  },
  "action": {
    "approve":       "Approve",
    "reject":        "Reject",
    "assignTo":      "Assign to",
    "convertToWO":   "Convert to Work Order",
    "close":         "Close",
    "addComment":    "Add comment"
  },
  "empty": {
    "list":          "No intervention requests found.",
    "listHint":      "Create a new request or adjust the filters.",
    "noComments":    "No comments for this request."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/locale-data/fr/ot.json (Ordres de Travail)
─────────────────────────────────────────────────────────────────────
```json
{
  "page": {
    "title":         "Ordres de travail",
    "titleDetail":   "Détail de l'ordre de travail",
    "titleNew":      "Nouvel ordre de travail",
    "titleEdit":     "Modifier l'ordre de travail"
  },
  "list": {
    "columns": {
      "number":      "N° OT",
      "title":       "Titre",
      "equipment":   "Équipement",
      "type":        "Type",
      "status":      "Statut",
      "priority":    "Priorité",
      "assignedTo":  "Assigné à",
      "plannedStart":"Début prévu",
      "plannedEnd":  "Fin prévue",
      "closedAt":    "Date clôture"
    },
    "filters": {
      "status":      "Statut",
      "type":        "Type",
      "priority":    "Priorité",
      "assignedTo":  "Assigné à"
    }
  },
  "detail": {
    "sections": {
      "general":     "Général",
      "planning":    "Planification",
      "labor":       "Main-d'œuvre",
      "parts":       "Pièces et matériaux",
      "delays":      "Délais",
      "closeout":    "Clôture",
      "history":     "Historique"
    },
    "fields": {
      "number":      "Numéro",
      "title":       "Titre",
      "description": "Description",
      "equipment":   "Équipement",
      "type":        "Type d'intervention",
      "priority":    "Priorité",
      "status":      "Statut",
      "assignedTo":  "Assigné à",
      "team":        "Équipe",
      "plannedStart":"Début planifié",
      "plannedEnd":  "Fin planifiée",
      "actualStart": "Début réel",
      "actualEnd":   "Fin réelle",
      "estimatedHours": "Heures estimées",
      "actualHours": "Heures réelles",
      "conclusion":  "Rapport de clôture"
    }
  },
  "form": {
    "title": {
      "label":       "Titre",
      "placeholder": "Description courte de l'intervention"
    },
    "description": {
      "label":       "Description",
      "placeholder": "Instructions détaillées pour l'exécutant"
    }
  },
  "status": {
    "draft":         "Brouillon",
    "planned":       "Planifié",
    "released":      "Lancé",
    "inProgress":    "En cours",
    "onHold":        "En attente",
    "completed":     "Terminé",
    "verified":      "Vérifié",
    "closed":        "Clôturé",
    "cancelled":     "Annulé"
  },
  "type": {
    "corrective":    "Correctif",
    "preventive":    "Préventif",
    "predictive":    "Prédictif",
    "improvement":   "Amélioration",
    "inspection":    "Inspection",
    "permit":        "Permis de travail"
  },
  "action": {
    "release":       "Lancer l'OT",
    "start":         "Démarrer",
    "pause":         "Mettre en pause",
    "complete":      "Marquer terminé",
    "verify":        "Vérifier et approuver",
    "close":         "Clôturer",
    "addLabor":      "Ajouter main-d'œuvre",
    "addPart":       "Ajouter une pièce",
    "addDelay":      "Enregistrer un délai"
  },
  "empty": {
    "list":          "Aucun ordre de travail.",
    "listHint":      "Créez un nouvel OT ou ajustez les filtres.",
    "noParts":       "Aucune pièce consommée.",
    "noLabor":       "Aucune main-d'œuvre enregistrée.",
    "noDelays":      "Aucun délai enregistré."
  }
}
```

─────────────────────────────────────────────────────────────────────
CREATE src/i18n/locale-data/en/ot.json
─────────────────────────────────────────────────────────────────────
```json
{
  "page": {
    "title":         "Work Orders",
    "titleDetail":   "Work order detail",
    "titleNew":      "New work order",
    "titleEdit":     "Edit work order"
  },
  "list": {
    "columns": {
      "number":      "WO No.",
      "title":       "Title",
      "equipment":   "Equipment",
      "type":        "Type",
      "status":      "Status",
      "priority":    "Priority",
      "assignedTo":  "Assigned to",
      "plannedStart":"Planned start",
      "plannedEnd":  "Planned end",
      "closedAt":    "Closed on"
    },
    "filters": {
      "status":      "Status",
      "type":        "Type",
      "priority":    "Priority",
      "assignedTo":  "Assigned to"
    }
  },
  "detail": {
    "sections": {
      "general":     "General",
      "planning":    "Planning",
      "labor":       "Labor",
      "parts":       "Parts & materials",
      "delays":      "Delays",
      "closeout":    "Closeout",
      "history":     "History"
    },
    "fields": {
      "number":      "Number",
      "title":       "Title",
      "description": "Description",
      "equipment":   "Equipment",
      "type":        "Work type",
      "priority":    "Priority",
      "status":      "Status",
      "assignedTo":  "Assigned to",
      "team":        "Team",
      "plannedStart":"Planned start",
      "plannedEnd":  "Planned end",
      "actualStart": "Actual start",
      "actualEnd":   "Actual end",
      "estimatedHours": "Estimated hours",
      "actualHours": "Actual hours",
      "conclusion":  "Closeout report"
    }
  },
  "form": {
    "title": {
      "label":       "Title",
      "placeholder": "Short description of the work"
    },
    "description": {
      "label":       "Description",
      "placeholder": "Detailed instructions for the technician"
    }
  },
  "status": {
    "draft":         "Draft",
    "planned":       "Planned",
    "released":      "Released",
    "inProgress":    "In progress",
    "onHold":        "On hold",
    "completed":     "Completed",
    "verified":      "Verified",
    "closed":        "Closed",
    "cancelled":     "Cancelled"
  },
  "type": {
    "corrective":    "Corrective",
    "preventive":    "Preventive",
    "predictive":    "Predictive",
    "improvement":   "Improvement",
    "inspection":    "Inspection",
    "permit":        "Work permit"
  },
  "action": {
    "release":       "Release WO",
    "start":         "Start",
    "pause":         "Put on hold",
    "complete":      "Mark complete",
    "verify":        "Verify and approve",
    "close":         "Close",
    "addLabor":      "Add labor",
    "addPart":       "Add part",
    "addDelay":      "Record delay"
  },
  "empty": {
    "list":          "No work orders found.",
    "listHint":      "Create a new work order or adjust the filters.",
    "noParts":       "No parts consumed.",
    "noLabor":       "No labor recorded.",
    "noDelays":      "No delays recorded."
  }
}
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- All 6 module JSON files exist and are valid JSON
- fr and en files for each module have the same key tree (same depth, same paths)
- pnpm run dev: switching to equipment module shows French labels from equipment.json
- No "[equipment:page.title]" fallback strings visible
```

---

### Supervisor Verification — Sprint S2

**V1 — Parity check: every fr key exists in en.**
Run this Node.js parity check on each module namespace pair:
```javascript
const fr = require('./src/i18n/locale-data/fr/equipment.json');
const en = require('./src/i18n/locale-data/en/equipment.json');

function getKeys(obj, prefix = '') {
  return Object.entries(obj).flatMap(([k, v]) =>
    typeof v === 'object' && !Array.isArray(v)
      ? getKeys(v, `${prefix}${k}.`)
      : [`${prefix}${k}`]
  );
}
const frKeys = new Set(getKeys(fr));
const enKeys = new Set(getKeys(en));
const onlyFr = [...frKeys].filter(k => !enKeys.has(k));
const onlyEn = [...enKeys].filter(k => !frKeys.has(k));
if (onlyFr.length || onlyEn.length) {
  console.error('Mismatch:', { onlyFr, onlyEn });
} else {
  console.log('equipment: fr/en parity OK');
}
```
Repeat for `di` and `ot`. All three must show `parity OK`.

**V2 — Module labels lazy-load without error.**
In `pnpm run dev`, navigate to the Equipment module URL (even if the component is
still a stub). Open DevTools → Network → filter for `.json`. You should see a request
for `fr/equipment.json` (or `en/equipment.json` if English is active) fired when the
module route is visited. If no request fires, lazy loading is broken; the namespace is
being loaded eagerly instead.

---

## Sprint S3 — Type Generation Verification and JSON Validation

### AI Agent Prompt

```
You are a TypeScript engineer. Sprints S1 and S2 have created all translation JSON
files. Your task is to write a lightweight script that validates JSON parity (fr vs en
key trees are identical) and checks that the eager JSON files match the shape expected
by types.ts.

─────────────────────────────────────────────────────────────────────
STEP 1 — Create src/i18n/__tests__/parity.test.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/i18n/__tests__/parity.test.ts
// Validates that every fr/ JSON file has an identical key tree in en/ and vice versa.
// Runs in vitest. Add to CI to catch missing translations before they reach production.

import { describe, it, expect } from "vitest";
import frCommon     from "../fr/common.json";
import enCommon     from "../en/common.json";
import frAuth       from "../fr/auth.json";
import enAuth       from "../en/auth.json";
import frErrors     from "../fr/errors.json";
import enErrors     from "../en/errors.json";
import frValidation from "../fr/validation.json";
import enValidation from "../en/validation.json";
import frShell      from "../fr/shell.json";
import enShell      from "../en/shell.json";

// formats.json is NOT checked for parity — it contains locale-specific
// tokens that are intentionally different between languages.

function collectKeys(obj: Record<string, unknown>, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([key, value]) =>
    typeof value === "object" && value !== null && !Array.isArray(value)
      ? collectKeys(value as Record<string, unknown>, `${prefix}${key}.`)
      : [`${prefix}${key}`]
  );
}

function checkParity(
  ns: string,
  fr: Record<string, unknown>,
  en: Record<string, unknown>
): void {
  const frKeys = new Set(collectKeys(fr));
  const enKeys = new Set(collectKeys(en));

  const missingInEn = [...frKeys].filter((k) => !enKeys.has(k));
  const missingInFr = [...enKeys].filter((k) => !frKeys.has(k));

  expect(missingInEn, `${ns}: keys in fr but not in en`).toEqual([]);
  expect(missingInFr, `${ns}: keys in en but not in fr`).toEqual([]);
}

describe("i18n JSON parity (fr ↔ en)", () => {
  it("common namespace is in parity", () => {
    checkParity("common", frCommon, enCommon);
  });

  it("auth namespace is in parity", () => {
    checkParity("auth", frAuth, enAuth);
  });

  it("errors namespace is in parity", () => {
    checkParity("errors", frErrors, enErrors);
  });

  it("validation namespace is in parity", () => {
    checkParity("validation", frValidation, enValidation);
  });

  it("shell namespace is in parity", () => {
    checkParity("shell", frShell, enShell);
  });
});
```

─────────────────────────────────────────────────────────────────────
STEP 2 — Create src/i18n/__tests__/json-valid.test.ts
─────────────────────────────────────────────────────────────────────
```typescript
// src/i18n/__tests__/json-valid.test.ts
// Verifies that each JSON file is importable and contains the expected top-level keys.

import { describe, it, expect } from "vitest";
import frCommon   from "../fr/common.json";
import enCommon   from "../en/common.json";
import frAuth     from "../fr/auth.json";
import enAuth     from "../en/auth.json";

describe("i18n JSON structure", () => {
  it("fr/common has required top-level keys", () => {
    expect(frCommon).toHaveProperty("app");
    expect(frCommon).toHaveProperty("action");
    expect(frCommon).toHaveProperty("status");
    expect(frCommon).toHaveProperty("label");
  });

  it("en/common has required top-level keys", () => {
    expect(enCommon).toHaveProperty("app");
    expect(enCommon).toHaveProperty("action");
    expect(enCommon).toHaveProperty("status");
    expect(enCommon).toHaveProperty("label");
  });

  it("fr/auth has login and session sections", () => {
    expect(frAuth).toHaveProperty("login");
    expect(frAuth).toHaveProperty("session");
    expect(frAuth).toHaveProperty("stepUp");
  });

  it("en/auth has login and session sections", () => {
    expect(enAuth).toHaveProperty("login");
    expect(enAuth).toHaveProperty("session");
    expect(enAuth).toHaveProperty("stepUp");
  });

  it("fr/common action.save is non-empty string", () => {
    expect(typeof frCommon.action.save).toBe("string");
    expect(frCommon.action.save.length).toBeGreaterThan(0);
  });

  it("en/common action.save is non-empty string", () => {
    expect(typeof enCommon.action.save).toBe("string");
    expect(enCommon.action.save.length).toBeGreaterThan(0);
  });
});
```

─────────────────────────────────────────────────────────────────────
ACCEPTANCE CRITERIA
─────────────────────────────────────────────────────────────────────
- `pnpm test src/i18n/__tests__/parity.test.ts` — all 5 parity tests pass
- `pnpm test src/i18n/__tests__/json-valid.test.ts` — all 6 structure tests pass
- `pnpm run typecheck` — 0 errors
- Any deliberate key removal in one language causes a parity test to fail with a
  clear message listing the missing key path
```

---

### Supervisor Verification — Sprint S3

**V1 — Parity tests all pass.**
Run `pnpm test src/i18n/__tests__/parity.test.ts`.
Expected output: 5 tests, all `✓`. If any test fails, it will print the key paths that
are in one language but not the other. Fix by adding the missing key to the appropriate
file.

**V2 — Deliberately break parity to confirm the test catches it.**
In `src/i18n/en/common.json`, temporarily remove the `"loading"` key from the
`"action"` object. Run the parity test again. It must now FAIL with a message
containing `action.loading`. Restore the key. This confirms the CI guard works.

**V3 — TypeScript compilation is clean.**
Run `pnpm run typecheck`. The JSON shape changes from this sprint should not introduce
any new TypeScript errors (the imports in `config.ts` and `types.ts` must resolve
correctly to the correct paths). If any path errors appear, check that `fr/common.json`
etc. are at the paths expected by the import statements in `config.ts`.

---

*End of Phase 1 · Sub-phase 05 · File 02*
