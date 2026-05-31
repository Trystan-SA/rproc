use super::*;

#[test]
fn parses_pmu_event_config_simple() {
    assert_eq!(parse_pmu_event_config("config=0x100003"), Some(0x100003));
    assert_eq!(parse_pmu_event_config("config=0x0"), Some(0));
}

#[test]
fn parses_pmu_event_config_with_extra_fields() {
    assert_eq!(
        parse_pmu_event_config("event=0x12,config=0xabcd,umask=0x1"),
        Some(0xabcd)
    );
}

#[test]
fn parses_pmu_event_config_missing() {
    assert_eq!(parse_pmu_event_config("event=0x12"), None);
}

#[test]
fn parses_first_cpu_single() {
    assert_eq!(parse_first_cpu("0"), Some(0));
    assert_eq!(parse_first_cpu("7"), Some(7));
}

#[test]
fn parses_first_cpu_range() {
    assert_eq!(parse_first_cpu("2-5"), Some(2));
}

#[test]
fn parses_first_cpu_list() {
    assert_eq!(parse_first_cpu("4,8,12"), Some(4));
}
