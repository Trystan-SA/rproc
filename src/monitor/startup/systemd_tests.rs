use super::*;

#[test]
fn parse_duration_single_units() {
    assert_eq!(parse_duration_to_ms("12ms"), Some(12));
    assert_eq!(parse_duration_to_ms("1.234s"), Some(1234));
    assert_eq!(parse_duration_to_ms("2min"), Some(120_000));
}

#[test]
fn parse_duration_compound() {
    // systemd-analyze emits space-separated compound durations.
    assert_eq!(parse_duration_to_ms("1min 2.345s"), Some(62_345));
    assert_eq!(parse_duration_to_ms("2h 3min 4s"), Some(7_384_000));
}

#[test]
fn parse_duration_rejects_empty_and_unitless() {
    assert_eq!(parse_duration_to_ms(""), None);
    assert_eq!(parse_duration_to_ms("   "), None);
    // A bare number with no recognized unit yields nothing.
    assert_eq!(parse_duration_to_ms("42"), None);
}

#[test]
fn parse_duration_stops_at_unknown_trailing_unit() {
    // Once at least one unit matched, a number with an unknown unit ends
    // parsing and returns what accumulated so far.
    assert_eq!(parse_duration_to_ms("1s 5x"), Some(1_000));
    // A trailing non-numeric token (no leading digit) is rejected outright.
    assert_eq!(parse_duration_to_ms("1s bogus"), None);
}
