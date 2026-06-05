use super::*;

const PDEV: &str = "0000:00:02.0";

fn client(busy_ns: u64, resident: Option<u64>) -> Client {
    Client { busy_ns, resident }
}

#[test]
fn sums_render_and_compute_for_matching_pdev() {
    let fdinfo = "\
drm-client-id:\t475
drm-pdev:\t0000:00:02.0
drm-engine-render:\t1000 ns
drm-engine-copy:\t50 ns
drm-engine-compute:\t250 ns
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((475, client(1250, None))));
}

#[test]
fn ignores_other_cards() {
    let fdinfo = "\
drm-client-id:\t1
drm-pdev:\t0000:03:00.0
drm-engine-render:\t999 ns
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), None);
}

#[test]
fn missing_client_id_yields_none() {
    let fdinfo = "\
drm-pdev:\t0000:00:02.0
drm-engine-render:\t1000 ns
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), None);
}

#[test]
fn no_pdev_line_yields_none() {
    let fdinfo = "drm-client-id:\t1\ndrm-engine-render:\t1000 ns\n";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), None);
}

#[test]
fn pdev_after_engine_lines_still_matches() {
    let fdinfo = "\
drm-client-id:\t7
drm-engine-render:\t300 ns
drm-pdev:\t0000:00:02.0
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((7, client(300, None))));
}

#[test]
fn malformed_engine_value_is_skipped() {
    let fdinfo = "\
drm-client-id:\t2
drm-pdev:\t0000:00:02.0
drm-engine-render:\tnonsense
drm-engine-compute:\t40 ns
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((2, client(40, None))));
}

#[test]
fn sums_resident_memory_across_regions() {
    let fdinfo = "\
drm-client-id:\t9
drm-pdev:\t0000:00:02.0
drm-engine-render:\t100 ns
drm-resident-system0:\t2048 KiB
drm-resident-stolen-system0:\t1 MiB
";
    assert_eq!(
        parse_fdinfo(fdinfo, PDEV),
        Some((9, client(100, Some(3 << 20))))
    );
}

#[test]
fn missing_resident_keys_yield_none_memory() {
    let fdinfo = "\
drm-client-id:\t3
drm-pdev:\t0000:00:02.0
drm-engine-render:\t10 ns
drm-total-system0:\t512 KiB
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((3, client(10, None))));
}

#[test]
fn malformed_resident_value_is_skipped() {
    let fdinfo = "\
drm-client-id:\t4
drm-pdev:\t0000:00:02.0
drm-resident-system0:\tnonsense
drm-resident-stolen-system0:\t4 KiB
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((4, client(0, Some(4096)))));
}

#[test]
fn parse_size_handles_units_and_bare_bytes() {
    assert_eq!(parse_size("512"), Some(512));
    assert_eq!(parse_size("2 KiB"), Some(2048));
    assert_eq!(parse_size("3 MiB"), Some(3 << 20));
    assert_eq!(parse_size("1 GiB"), Some(1 << 30));
    assert_eq!(parse_size("1 parsec"), None);
    assert_eq!(parse_size(""), None);
}
