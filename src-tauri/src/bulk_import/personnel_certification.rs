//! Certificate import ("Habilitations / certificats"): a person's certificates by certificate type.
//!
//! Certificates come in "to check" (pending): they count for no qualification or safety gate until
//! a trn.certify holder verifies them. A line updates the certificate of the same person and type
//! with the same certificate number (else the same issue date; else the only one of that type);
//! otherwise a new certificate is recorded. A blank cell never clears a value, and a changed
//! certificate goes back to "to check". Nothing is ever deleted.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use serde_json::json;

use super::org_tree::key;
use super::personnel_skill::{people_by_code, resolve_person};
use super::team::app_error_text;
use super::{parse_date, Column, EntitySpec, RowApplied, RowMessage, RowOutcome, StagedRow};
use crate::errors::{AppError, AppResult};
use crate::qualification::domain::PersonnelCertificationUpsertInput;

pub const SPEC: EntitySpec = EntitySpec {
    code: "personnel_certification",
    label: "Habilitations / certificats",
    order: 3,
    needs: "Personnel, et les types de certificat (Formation & habilitations).",
    permission: crate::rbac::permissions::TRN_CERTIFY,
    columns: &[
        Column {
            key: "employee_code",
            header: "matricule",
            required: true,
            aliases: &["employee_code", "matricule_personnel", "code_personnel"],
            hint: "Matricule d'une personne du registre du personnel.",
        },
        Column {
            key: "type_code",
            header: "type de certificat",
            required: true,
            aliases: &["type", "code_type", "certificat", "habilitation", "type_code"],
            hint: "Code d'un type de certificat existant.",
        },
        Column {
            key: "issued_at",
            header: "date d'obtention",
            required: false,
            aliases: &["obtention", "date_obtention", "issued_at"],
            hint: "JJ/MM/AAAA.",
        },
        Column {
            key: "expires_at",
            header: "date d'expiration",
            required: false,
            aliases: &["expiration", "date_expiration", "expires_at"],
            hint: "JJ/MM/AAAA. Obligatoire quand le type de certificat a une durée de validité.",
        },
        Column {
            key: "issuing_body",
            header: "organisme",
            required: false,
            aliases: &["organisme_emetteur", "emetteur", "issuing_body"],
            hint: "Organisme qui a délivré le certificat.",
        },
        Column {
            key: "certificate_ref",
            header: "n° de certificat",
            required: false,
            aliases: &["n_de_certificat", "numero", "numero_de_certificat", "certificate_ref"],
            hint: "Numéro du certificat. Sert aussi à retrouver un certificat existant.",
        },
    ],
};

struct CertType {
    id: i64,
    has_validity: bool,
}

struct Stored {
    id: i64,
    row_version: i64,
    issued: Option<NaiveDate>,
    expires: Option<NaiveDate>,
    body: Option<String>,
    cert_ref: Option<String>,
    status: String,
}

fn stored_date(raw: Option<String>) -> Option<NaiveDate> {
    raw.and_then(|s| NaiveDate::parse_from_str(s.get(..10)?, "%Y-%m-%d").ok())
}

fn show(d: Option<NaiveDate>) -> String {
    d.map(|d| d.format("%d/%m/%Y").to_string()).unwrap_or_default()
}

pub async fn validate(db: &DatabaseConnection, rows: &[HashMap<String, String>]) -> AppResult<Vec<RowOutcome>> {
    let (people, ambiguous_people) = people_by_code(db).await?;

    let mut types: HashMap<String, CertType> = HashMap::new();
    let mut ambiguous_types: HashSet<String> = HashSet::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, code, default_validity_months FROM certification_types".to_string(),
        ))
        .await?
    {
        let code: String = r.try_get("", "code")?;
        let validity: Option<i64> = r.try_get("", "default_validity_months")?;
        let previous = types.insert(
            key(&code),
            CertType {
                id: r.try_get("", "id")?,
                has_validity: validity.is_some_and(|m| m > 0),
            },
        );
        if previous.is_some() {
            ambiguous_types.insert(key(&code));
        }
    }

    let mut certs: HashMap<(i64, i64), Vec<Stored>> = HashMap::new();
    for r in db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT id, personnel_id, certification_type_id, issued_at, expires_at, issuing_body, \
                    certificate_ref, verification_status, row_version \
             FROM personnel_certifications ORDER BY id"
                .to_string(),
        ))
        .await?
    {
        certs
            .entry((r.try_get("", "personnel_id")?, r.try_get("", "certification_type_id")?))
            .or_default()
            .push(Stored {
                id: r.try_get("", "id")?,
                row_version: r.try_get("", "row_version")?,
                issued: stored_date(r.try_get("", "issued_at")?),
                expires: stored_date(r.try_get("", "expires_at")?),
                body: r.try_get("", "issuing_body")?,
                cert_ref: r.try_get("", "certificate_ref")?,
                status: r.try_get("", "verification_status")?,
            });
    }

    // Lines of the file by the certificate they write: an existing one, or a new one told apart by
    // its number, else its issue date.
    let mut first_row_of: HashMap<(i64, i64, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for (i, raw) in rows.iter().enumerate() {
        let mut messages = Vec::new();
        let cell = |k: &str| raw.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
        let person = resolve_person(
            cell("employee_code"),
            &people,
            &ambiguous_people,
            "CERT_IMPORT",
            &mut messages,
        );
        let cert_type = match cell("type_code") {
            None => {
                messages.push(RowMessage::error(
                    "CERT_IMPORT_TYPE",
                    "Le type de certificat est obligatoire.",
                ));
                None
            }
            Some(t) if ambiguous_types.contains(&key(t)) => {
                messages.push(RowMessage::error(
                    "CERT_IMPORT_TYPE_AMBIGUOUS",
                    format!("Plusieurs types de certificat ont le code « {t} » à la casse près."),
                ));
                None
            }
            Some(t) => {
                let found = types.get(&key(t));
                if found.is_none() {
                    messages.push(RowMessage::error(
                        "CERT_IMPORT_TYPE",
                        format!(
                            "Le type de certificat « {t} » n'existe pas : créez-le d'abord dans Formation & habilitations."
                        ),
                    ));
                }
                found
            }
        };
        let mut date = |k: &str, label: &str| match parse_date(cell(k)) {
            Ok(d) => d,
            Err(()) => {
                messages.push(RowMessage::error(
                    "CERT_IMPORT_DATE",
                    format!("La {label} doit être une date JJ/MM/AAAA."),
                ));
                None
            }
        };
        let issued = date("issued_at", "date d'obtention");
        let expires = date("expires_at", "date d'expiration");
        let body = cell("issuing_body").map(str::to_string);
        let cert_ref = cell("certificate_ref").map(str::to_string);

        // The existing certificate this line is about.
        let mut matched: Option<usize> = None;
        if let (Some(p), Some(t)) = (person, cert_type) {
            let list = certs.get(&(p, t.id)).map(Vec::as_slice).unwrap_or_default();
            let same_ref = |c: &Stored| {
                cert_ref
                    .as_ref()
                    .is_some_and(|r| c.cert_ref.as_deref().map(key) == Some(key(r)))
            };
            if list.iter().filter(|c| same_ref(c)).count() > 1 {
                messages.push(RowMessage::error(
                    "CERT_IMPORT_WHICH",
                    "Cette personne a plusieurs certificats de ce type avec ce numéro : corrigez-les dans Formation & habilitations.",
                ));
            }
            let by_ref = list.iter().position(same_ref);
            let by_date = || {
                issued.and_then(|d| {
                    list.iter()
                        .position(|c| c.issued == Some(d) && (cert_ref.is_none() || c.cert_ref.is_none()))
                })
            };
            matched = by_ref.or_else(by_date);
            if matched.is_none() && cert_ref.is_none() && issued.is_none() {
                match list.len() {
                    0 => {}
                    1 => matched = Some(0),
                    _ => messages.push(RowMessage::error(
                        "CERT_IMPORT_WHICH",
                        "Cette personne a plusieurs certificats de ce type : indiquez le n° de certificat ou la date d'obtention.",
                    )),
                }
            }
            let which = match (matched, &cert_ref, issued) {
                (Some(m), _, _) => format!("#{m}"),
                (None, Some(r), _) => format!("n:{}", key(r)),
                (None, None, Some(d)) => format!("d:{d}"),
                (None, None, None) => "-".to_string(),
            };
            let k = (p, t.id, which);
            if let Some(first) = first_row_of.get(&k) {
                messages.push(RowMessage::error(
                    "CERT_IMPORT_DUPLICATE",
                    format!("Ce certificat apparaît déjà à la ligne {} du fichier.", first + 2),
                ));
            } else {
                first_row_of.insert(k, i);
            }
        }
        let stored = matched.and_then(|m| {
            let (p, t) = (person?, cert_type?);
            certs.get(&(p, t.id)).map(|list| &list[m])
        });

        // The certificate as it will be (a blank cell keeps the stored value).
        let after_issued = issued.or(stored.and_then(|s| s.issued));
        let after_expires = expires.or(stored.and_then(|s| s.expires));
        if let (Some(a), Some(b)) = (after_issued, after_expires) {
            if b < a {
                messages.push(RowMessage::error(
                    "CERT_IMPORT_DATES",
                    "La date d'expiration est avant la date d'obtention.",
                ));
            }
        }
        if cert_type.is_some_and(|t| t.has_validity) && after_expires.is_none() {
            messages.push(RowMessage::error(
                "CERT_IMPORT_EXPIRY_REQUIRED",
                "Ce type de certificat a une durée de validité : la date d'expiration est obligatoire.",
            ));
        }

        let changes = stored.is_none_or(|s| {
            issued.is_some_and(|d| Some(d) != s.issued)
                || expires.is_some_and(|d| Some(d) != s.expires)
                || body.as_ref().is_some_and(|b| Some(b) != s.body.as_ref())
                || cert_ref.as_ref().is_some_and(|r| Some(r) != s.cert_ref.as_ref())
        });
        let has_error = messages.iter().any(|m| m.severity == "error");
        let action = match (has_error, stored, changes) {
            (true, _, _) | (false, Some(_), false) => "skip",
            (false, None, _) => "create",
            (false, Some(_), true) => "update",
        };
        if action == "update" && stored.is_some_and(|s| s.status == "verified") {
            messages.push(RowMessage::warning(
                "CERT_IMPORT_REVERIFY",
                "Le certificat change : il repasse « à vérifier » et ne compte plus pour les habilitations avant vérification.",
            ));
        }
        out.push(RowOutcome {
            action,
            normalized: json!({
                "personnel_id": person,
                "certification_type_id": cert_type.map(|t| t.id),
                "id": stored.map(|s| s.id),
                "row_version": stored.map(|s| s.row_version),
                "issued_at": after_issued.map(|d| d.to_string()),
                "expires_at": after_expires.map(|d| d.to_string()),
                "issuing_body": body.or_else(|| stored.and_then(|s| s.body.clone())),
                "certificate_ref": cert_ref.or_else(|| stored.and_then(|s| s.cert_ref.clone())),
            }),
            messages,
        });
    }
    Ok(out)
}

pub async fn apply(db: &DatabaseConnection, rows: &[StagedRow]) -> AppResult<Vec<RowApplied>> {
    let mut out = Vec::new();
    for row in rows {
        let n = &row.normalized;
        let text = |k: &str| n[k].as_str().map(str::to_string);
        let (status, message) = match (
            row.action.as_str(),
            n["personnel_id"].as_i64(),
            n["certification_type_id"].as_i64(),
        ) {
            ("create" | "update", Some(p), Some(t)) => {
                let input = PersonnelCertificationUpsertInput {
                    id: n["id"].as_i64(),
                    personnel_id: p,
                    certification_type_id: t,
                    issued_at: text("issued_at"),
                    expires_at: text("expires_at"),
                    issuing_body: text("issuing_body"),
                    certificate_ref: text("certificate_ref"),
                    verification_status: "pending".to_string(),
                    expected_row_version: n["row_version"].as_i64(),
                };
                match crate::qualification::queries::upsert_personnel_certification(db, input).await {
                    Ok(_) => (
                        if row.action == "create" { "created" } else { "updated" },
                        String::new(),
                    ),
                    Err(AppError::Database(e)) => return Err(AppError::Database(e)),
                    Err(e) => ("failed", app_error_text(&e)),
                }
            }
            ("create" | "update", _, _) => ("failed", "Ligne incomplète.".to_string()),
            _ => ("skipped", "Déjà à jour.".to_string()),
        };
        out.push(RowApplied {
            row_id: row.id,
            status,
            message,
        });
    }
    Ok(out)
}

pub async fn export(db: &DatabaseConnection) -> AppResult<Vec<Vec<String>>> {
    let rows = db
        .query_all(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT p.employee_code, ct.code, pc.issued_at, pc.expires_at, pc.issuing_body, pc.certificate_ref \
             FROM personnel_certifications pc \
             JOIN personnel p ON p.id = pc.personnel_id \
             JOIN certification_types ct ON ct.id = pc.certification_type_id \
             ORDER BY p.employee_code, ct.code, pc.issued_at"
                .to_string(),
        ))
        .await?;
    rows.iter()
        .map(|r| {
            Ok(vec![
                r.try_get("", "employee_code")?,
                r.try_get("", "code")?,
                show(stored_date(r.try_get("", "issued_at")?)),
                show(stored_date(r.try_get("", "expires_at")?)),
                r.try_get::<Option<String>>("", "issuing_body")?.unwrap_or_default(),
                r.try_get::<Option<String>>("", "certificate_ref")?.unwrap_or_default(),
            ])
        })
        .collect()
}
