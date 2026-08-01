#[cfg(test)]
mod errors_tests {
    use crate::errors::{AppError, AppResult};

    // ── Variant construction ────────────────────────────────────────────

    #[test]
    fn auth_error_preserves_message() {
        let err = AppError::Auth("bad token".into());
        assert_eq!(err.to_string(), "Authentication error: bad token");
    }

    #[test]
    fn not_found_formats_entity_and_id() {
        let err = AppError::NotFound {
            entity: "Equipment".into(),
            id: "42".into(),
        };
        assert!(err.to_string().contains("Equipment"));
        assert!(err.to_string().contains("42"));
    }

    #[test]
    fn validation_failed_contains_all_messages() {
        let msgs = vec!["field required".into(), "too long".into()];
        let err = AppError::ValidationFailed(msgs);
        let s = err.to_string();
        assert!(s.contains("field required"));
        assert!(s.contains("too long"));
        assert!(!s.contains("[\""));
    }

    #[test]
    fn validation_failed_serializes_joined_message_and_details() {
        let err = AppError::ValidationFailed(vec!["a".into(), "b".into()]);
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "VALIDATION_FAILED");
        assert_eq!(json["message"], "Validation failed: a; b");
        assert!(json["details"].as_array().unwrap().len() == 2);
    }

    #[test]
    fn org_validation_failed_serializes_structured_details() {
        use crate::errors::{issue_params, AppValidationIssue};
        let err = AppError::OrgValidationFailed(vec![AppValidationIssue::error(
            "ORG_MOVE_CYCLE",
            "cannot move under itself",
            issue_params(&[]),
        )]);
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "VALIDATION_FAILED");
        assert!(json["message"].as_str().unwrap().contains("cannot move under itself"));
        assert_eq!(json["details"][0]["code"], "ORG_MOVE_CYCLE");
    }

    #[test]
    fn permission_denied_includes_action_and_resource() {
        let err = AppError::Permission {
            action: "delete".into(),
            resource: "work_order".into(),
        };
        let s = err.to_string();
        assert!(s.contains("delete"));
        assert!(s.contains("work_order"));
    }

    #[test]
    fn io_error_converts_via_from() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let err: AppError = io.into();
        assert!(matches!(err, AppError::Io(_)));
    }

    #[test]
    fn serialization_error_converts_via_from() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{invalid");
        let err: AppError = bad.unwrap_err().into();
        assert!(matches!(err, AppError::Serialization(_)));
    }

    #[test]
    fn internal_error_converts_via_anyhow() {
        let err: AppError = anyhow::anyhow!("unexpected crash").into();
        assert!(matches!(err, AppError::Internal(_)));
    }

    // ── Serialization / IPC boundary ────────────────────────────────────

    #[test]
    fn serialize_produces_code_and_message_fields() {
        let err = AppError::Auth("expired".into());
        let json = serde_json::to_value(&err).expect("serialize");
        assert_eq!(json["code"], "AUTH_ERROR");
        assert!(json["message"].as_str().unwrap().contains("expired"));
    }

    #[test]
    fn all_variants_map_to_correct_error_codes() {
        let cases: Vec<(AppError, &str)> = vec![
            (AppError::Database(sea_orm::DbErr::Custom("x".into())), "DATABASE_ERROR"),
            (AppError::Auth("x".into()), "AUTH_ERROR"),
            (AppError::TenantScopeViolation("x".into()), "TENANT_SCOPE_VIOLATION"),
            (AppError::SessionClaimInvalid("x".into()), "SESSION_CLAIM_INVALID"),
            (
                AppError::NotFound {
                    entity: "e".into(),
                    id: "1".into(),
                },
                "NOT_FOUND",
            ),
            (AppError::ValidationFailed(vec![]), "VALIDATION_FAILED"),
            (AppError::OrgValidationFailed(vec![]), "VALIDATION_FAILED"),
            (AppError::SyncError("x".into()), "SYNC_ERROR"),
            (
                AppError::Io(std::io::Error::new(std::io::ErrorKind::Other, "x")),
                "IO_ERROR",
            ),
            (
                AppError::Serialization(serde_json::from_str::<()>("bad").unwrap_err()),
                "SERIALIZATION_ERROR",
            ),
            (
                AppError::Permission {
                    action: "a".into(),
                    resource: "r".into(),
                },
                "PERMISSION_DENIED",
            ),
            (AppError::PermissionDenied("x".into()), "PERMISSION_DENIED"),
            (
                AppError::LicenseDenied {
                    reason_code: "entitlement_violation".into(),
                    message: "blocked".into(),
                },
                "LICENSE_DENIED",
            ),
            (AppError::StepUpRequired, "STEP_UP_REQUIRED"),
            (
                AppError::AccountLocked {
                    until: "2026-01-01T00:00:00Z".into(),
                },
                "ACCOUNT_LOCKED",
            ),
            (AppError::SessionLocked("idle".into()), "SESSION_LOCKED"),
            (AppError::Internal(anyhow::anyhow!("boom")), "INTERNAL_ERROR"),
        ];

        for (err, expected_code) in cases {
            let json = serde_json::to_value(&err).expect("serialize");
            assert_eq!(json["code"].as_str().unwrap(), expected_code, "wrong code for {err:?}");
        }
    }

    #[test]
    fn internal_error_surfaces_capped_french_prefix() {
        let err = AppError::Internal(anyhow::anyhow!("mapping failure"));
        let json = serde_json::to_value(&err).expect("serialize");
        let msg = json["message"].as_str().unwrap();
        assert!(
            msg.starts_with("Erreur interne"),
            "expected French internal prefix, got: {msg}"
        );
        assert!(msg.contains("mapping failure"));
    }

    #[test]
    fn non_internal_errors_do_expose_their_messages() {
        let err = AppError::Auth("token expired".into());
        let json = serde_json::to_value(&err).expect("serialize");
        assert!(json["message"].as_str().unwrap().contains("token expired"));
    }

    // ── AppResult alias ─────────────────────────────────────────────────

    #[test]
    fn app_result_ok_unwraps() {
        let res: AppResult<u32> = Ok(42);
        assert_eq!(res.unwrap(), 42);
    }

    #[test]
    fn app_result_err_carries_app_error() {
        let res: AppResult<()> = Err(AppError::Auth("no".into()));
        assert!(res.is_err());
    }
}
