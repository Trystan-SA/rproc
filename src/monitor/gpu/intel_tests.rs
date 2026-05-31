use super::*;

const PDEV: &str = "0000:00:02.0";

#[test]
fn sums_render_and_compute_for_matching_pdev() {
    let fdinfo = "\
drm-client-id:\t475
drm-pdev:\t0000:00:02.0
drm-engine-render:\t1000 ns
drm-engine-copy:\t50 ns
drm-engine-compute:\t250 ns
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((475, 1250)));
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
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((7, 300)));
}

#[test]
fn malformed_engine_value_is_skipped() {
    let fdinfo = "\
drm-client-id:\t2
drm-pdev:\t0000:00:02.0
drm-engine-render:\tnonsense
drm-engine-compute:\t40 ns
";
    assert_eq!(parse_fdinfo(fdinfo, PDEV), Some((2, 40)));
}
