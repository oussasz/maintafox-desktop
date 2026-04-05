#[cfg(test)]
mod window_tests {
    use crate::window::WindowState;

    #[test]
    fn default_window_state_has_expected_values() {
        let s = WindowState::default();
        assert_eq!(s.width, 1280);
        assert_eq!(s.height, 800);
        assert!(!s.maximized);
    }

    #[test]
    fn window_state_round_trips_json() {
        let original = WindowState {
            width: 1400,
            height: 900,
            x: 100,
            y: 50,
            maximized: false,
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let restored: WindowState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.width, original.width);
        assert_eq!(restored.height, original.height);
        assert_eq!(restored.x, original.x);
        assert_eq!(restored.y, original.y);
        assert_eq!(restored.maximized, original.maximized);
    }

    #[test]
    fn window_state_missing_fields_fall_back_to_defaults() {
        let partial = r#"{"width":1600,"height":1000,"x":0,"y":0,"maximized":false}"#;
        let state: WindowState = serde_json::from_str(partial).expect("deserialize partial");
        assert_eq!(state.width, 1600);
    }

    #[test]
    fn window_state_corrupt_json_falls_back_cleanly() {
        let bad = r#"{"width":"not_a_number"}"#;
        let result = serde_json::from_str::<WindowState>(bad);
        assert!(result.is_err(), "should fail to deserialize");
        let state: WindowState = serde_json::from_str(bad).unwrap_or_default();
        assert_eq!(state.width, 1280);
    }
}
