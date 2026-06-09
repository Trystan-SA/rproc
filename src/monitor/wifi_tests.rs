use super::*;

const SAMPLE: &str = "\
Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
wlp130s0: 0000   60.  -50.  -256        0      0      0      0    161        0
";

#[test]
fn parses_link_and_level() {
    let map = parse_wireless(SAMPLE);
    let s = map.get("wlp130s0").expect("interface parsed");
    assert_eq!(s.link_quality, 60.0);
    assert_eq!(s.signal_dbm, -50.0);
}

#[test]
fn derives_quality_pct_from_dbm() {
    assert_eq!(dbm_to_pct(-50.0), 100.0);
    assert_eq!(dbm_to_pct(-75.0), 50.0);
    assert_eq!(dbm_to_pct(-100.0), 0.0);
    // Clamped beyond the usable range.
    assert_eq!(dbm_to_pct(-30.0), 100.0);
    assert_eq!(dbm_to_pct(-120.0), 0.0);
}

#[test]
fn skips_headers_and_blank_interfaces() {
    let map = parse_wireless(SAMPLE);
    assert_eq!(map.len(), 1);
}

#[test]
fn empty_input_yields_no_entries() {
    assert!(parse_wireless("").is_empty());
}

#[test]
fn ignores_rows_missing_fields() {
    let map = parse_wireless("wlp0s0: 0000\n");
    assert!(map.is_empty());
}
