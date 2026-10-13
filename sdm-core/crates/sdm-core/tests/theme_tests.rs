use sdm_core::Theme;

#[test]
fn theme_round_trips_through_json() {
    let theme = Theme::new(
        "test", "Test", true, "SDM", "#111111", "#222222", "#333333", "#FFFFFF", "#DDDDDD",
        "#BBBBBB", "#4488FF", "#444444", "#33CC66", "#FFAA33", "#888888", "#FF4444", "#4488FF",
        "#33CC66", "#66CCFF", "#4488FF", "#AA66FF", "#0A0A0A", "#4488FF",
    );
    let data = serde_json::to_vec(&theme).unwrap();
    let decoded: Theme = serde_json::from_slice(&data).unwrap();
    assert_eq!(decoded, theme);
}
