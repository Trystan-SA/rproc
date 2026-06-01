use super::*;

#[test]
fn format_pct_low_values_show_decimal() {
    // Below 0.05 collapses to 0% so the column doesn't churn between
    // 0.0% / 0.1% for idle rows on every frame.
    assert_eq!(format_pct(0.0), "0%");
    assert_eq!(format_pct(0.04), "0%");
    assert_eq!(format_pct(0.5), "0.5%");
    assert_eq!(format_pct(9.9), "9.9%");
}

#[test]
fn format_pct_high_values_round_to_int() {
    assert_eq!(format_pct(10.0), "10%");
    assert_eq!(format_pct(42.49), "42%");
    assert_eq!(format_pct(100.0), "100%");
}

#[test]
fn status_color_known_states_distinct_from_default() {
    // Regression: every known status string should map to a non-default
    // colour (otherwise the column loses its visual cue).
    let default = theme::text();
    assert_ne!(status_color("Running"), default);
    assert_ne!(status_color("Idle"), default);
    assert_ne!(status_color("Stopped"), default);
    assert_ne!(status_color("Zombie"), default);
    // Unknown still hits default — guard against accidental match-all.
    assert_eq!(status_color("XyzUnknown"), default);
}
