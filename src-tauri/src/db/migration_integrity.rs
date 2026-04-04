#[cfg(test)]
mod tests {
    /// Verifies migration names follow m<YYYYMMDD>_<NNNNNN>_<name> convention
    /// and that the sequence numbers are contiguous starting from 000001.
    #[test]
    fn migration_names_follow_convention() {
        let names = migration_name_list();
        for name in &names {
            assert!(
                name.starts_with('m'),
                "Migration name must start with 'm': {}",
                name
            );
            let parts: Vec<&str> = name.splitn(3, '_').collect();
            assert_eq!(
                parts.len(),
                3,
                "Migration name must have 3 underscore-separated segments: {}",
                name
            );
            let date_part = parts[0].trim_start_matches('m');
            assert_eq!(
                date_part.len(),
                8,
                "Date segment must be 8 digits (YYYYMMDD): {}",
                name
            );
            assert!(
                date_part.chars().all(|c| c.is_ascii_digit()),
                "Date segment must be all digits: {}",
                name
            );
            let seq_part = parts[1];
            assert_eq!(
                seq_part.len(),
                6,
                "Sequence segment must be 6 digits: {}",
                name
            );
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

        for (i, seq) in seqs.iter().enumerate() {
            assert_eq!(
                *seq,
                (i + 1) as u32,
                "Migration sequence must be contiguous starting at 1. \\n                 Expected {} at position {}, found {}",
                i + 1,
                i,
                seq
            );
        }
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
        ]
    }
}
