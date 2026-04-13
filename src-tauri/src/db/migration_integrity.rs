#[cfg(test)]
mod tests {
    /// Verifies migration names follow m<YYYYMMDD>_<NNNNNN>_<name> convention
    /// and that the sequence numbers are contiguous starting from 000001.
    #[test]
    fn migration_names_follow_convention() {
        let names = migration_name_list();
        for name in &names {
            assert!(name.starts_with('m'), "Migration name must start with 'm': {}", name);
            let parts: Vec<&str> = name.splitn(3, '_').collect();
            assert_eq!(
                parts.len(),
                3,
                "Migration name must have 3 underscore-separated segments: {}",
                name
            );
            let date_part = parts[0].trim_start_matches('m');
            assert_eq!(date_part.len(), 8, "Date segment must be 8 digits (YYYYMMDD): {}", name);
            assert!(
                date_part.chars().all(|c| c.is_ascii_digit()),
                "Date segment must be all digits: {}",
                name
            );
            let seq_part = parts[1];
            assert_eq!(seq_part.len(), 6, "Sequence segment must be 6 digits: {}", name);
            assert!(
                seq_part.chars().all(|c| c.is_ascii_digit()),
                "Sequence segment must be all digits: {}",
                name
            );
        }
    }

    #[test]
    fn migration_sequence_numbers_are_contiguous() {
        let names = migration_name_list();
        let mut seqs: Vec<u32> = names
            .iter()
            .map(|name| {
                let parts: Vec<&str> = name.splitn(3, '_').collect();
                parts[1].parse::<u32>().expect("Sequence must be numeric")
            })
            .collect();
        seqs.sort();

        // Migration 016 was intentionally skipped during the DI domain sprint.
        // Verify the full expected set: 1..=35 minus 16.
        let known_skips: &[u32] = &[16];
        let expected: Vec<u32> = (1..=35).filter(|n| !known_skips.contains(n)).collect();

        assert_eq!(
            seqs, expected,
            "Migration sequence numbers do not match the expected set (1..=35, skip 16)"
        );
    }

    /// Returns the list of all registered migration name strings.
    /// Must be kept in sync with migrations/mod.rs.
    fn migration_name_list() -> Vec<String> {
        vec![
            "m20260401_000001_system_tables".into(),
            "m20260401_000002_user_tables".into(),
            "m20260402_000003_reference_domains".into(),
            "m20260402_000004_org_schema".into(),
            "m20260402_000005_equipment_schema".into(),
            "m20260402_000006_teams_and_skills".into(),
            "m20260404_000007_settings_tables".into(),
            "m20260401_000008_backup_tables".into(),
            "m20260406_000009_org_audit_trail".into(),
            "m20260401_000010_asset_registry_core".into(),
            "m20260401_000011_asset_lifecycle_meter_docs".into(),
            "m20260401_000012_asset_import_and_audit".into(),
            "m20260401_000013_reference_domains_core".into(),
            "m20260401_000014_reference_governance_maps".into(),
            "m20260401_000015_reference_aliases_and_imports".into(),
            "m20260401_000017_di_domain_core".into(),
            "m20260401_000018_di_review_events".into(),
            "m20260401_000019_di_attachments_sla".into(),
            "m20260401_000020_di_change_events".into(),
            "m20260408_000021_org_node_type_color".into(),
            "m20260409_000022_wo_domain_core".into(),
            "m20260410_000023_wo_execution_sub_entities".into(),
            "m20260410_000024_wo_shift_column".into(),
            "m20260410_000025_wo_closeout_and_attachments".into(),
            "m20260411_000026_wo_change_events".into(),
            "m20260411_000027_wo_conclusion_column".into(),
            "m20260412_000028_rbac_scope_model".into(),
            "m20260412_000029_permission_catalog".into(),
            "m20260412_000030_admin_change_events".into(),
            "m20260412_000031_rbac_settings_and_lockout".into(),
            "m20260413_000032_rbac_hardening".into(),
            "m20260413_000033_password_policy_settings".into(),
            "m20260901_000034_notification_core".into(),
            "m20261001_000035_archive_core".into(),
        ]
    }
}
