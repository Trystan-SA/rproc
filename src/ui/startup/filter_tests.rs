use super::*;

#[test]
fn parse_time_filter_with_no_prefix_means_ge() {
    // Bare number = ">=" so users can type "0.5" without thinking about
    // operators for the common "show slow startups" case.
    let (op, v) = parse_time_filter("0.5").unwrap();
    assert!(matches!(op, Op::Ge));
    assert_eq!(v, 0.5);
}

#[test]
fn parse_time_filter_all_operator_prefixes() {
    assert!(matches!(parse_time_filter(">1").unwrap().0, Op::Gt));
    assert!(matches!(parse_time_filter("<1").unwrap().0, Op::Lt));
    assert!(matches!(parse_time_filter(">=1").unwrap().0, Op::Ge));
    assert!(matches!(parse_time_filter("<=1").unwrap().0, Op::Le));
    assert!(matches!(parse_time_filter("=1").unwrap().0, Op::Eq));
}

#[test]
fn parse_time_filter_strips_trailing_unit_suffix() {
    // The hint text suggests `>0.5` but users often type `>0.5s`.
    let (_, v) = parse_time_filter(">0.5s").unwrap();
    assert_eq!(v, 0.5);
}

#[test]
fn parse_time_filter_rejects_non_numeric() {
    assert!(parse_time_filter("hello").is_none());
    assert!(parse_time_filter("").is_none());
    assert!(parse_time_filter(">").is_none());
}

#[test]
fn op_matches_inclusive_vs_exclusive() {
    assert!(Op::Ge.matches(1.0, 1.0));
    assert!(!Op::Gt.matches(1.0, 1.0));
    assert!(Op::Le.matches(1.0, 1.0));
    assert!(!Op::Lt.matches(1.0, 1.0));
}

#[test]
fn op_matches_eq_uses_epsilon() {
    // Eq tolerates 50 ms drift — the boot-time series is reported with
    // ~100 ms granularity, so strict equality would never fire.
    assert!(Op::Eq.matches(1.02, 1.0));
    assert!(Op::Eq.matches(0.96, 1.0));
    assert!(!Op::Eq.matches(1.06, 1.0));
    assert!(!Op::Eq.matches(0.93, 1.0));
}

#[test]
fn format_boot_time_none_dash() {
    assert_eq!(format_boot_time(None), "—");
}

#[test]
fn format_boot_time_unit_boundaries() {
    assert_eq!(format_boot_time(Some(0)), "<1 ms");
    assert_eq!(format_boot_time(Some(1)), "1 ms");
    assert_eq!(format_boot_time(Some(999)), "999 ms");
    assert_eq!(format_boot_time(Some(1_000)), "1.00 s");
    assert_eq!(format_boot_time(Some(12_345)), "12.35 s");
    assert_eq!(format_boot_time(Some(59_999)), "60.00 s");
    assert_eq!(format_boot_time(Some(60_000)), "1m 0s");
    assert_eq!(format_boot_time(Some(125_000)), "2m 5s");
}
