use super::*;

#[test]
fn format_bytes_picks_right_unit() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1 KB");
    assert_eq!(format_bytes(1_048_576), "1.0 MB");
    assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
    assert_eq!(format_bytes(1_099_511_627_776), "1.0 TB");
}

#[test]
fn format_bytes_boundary_just_below_kb_stays_in_bytes() {
    assert_eq!(format_bytes(1023), "1023 B");
}

#[test]
fn format_bps_picks_right_unit() {
    // bps uses powers of 1000 (SI), unlike format_bytes which uses 1024.
    assert_eq!(format_bps(0.0), "0 B/s");
    assert_eq!(format_bps(999.0), "999 B/s");
    assert_eq!(format_bps(1_000.0), "1 KB/s");
    assert_eq!(format_bps(1_000_000.0), "1.0 MB/s");
    assert_eq!(format_bps(1_000_000_000.0), "1.00 GB/s");
}

#[test]
fn format_duration_picks_the_largest_unit_present() {
    assert_eq!(format_duration(0), "0s");
    assert_eq!(format_duration(45), "45s");
    assert_eq!(format_duration(60), "1m 0s");
    assert_eq!(format_duration(125), "2m 5s");
    assert_eq!(format_duration(3600), "1h 0m 0s");
    assert_eq!(format_duration(3725), "1h 2m 5s");
    assert_eq!(format_duration(86_400), "1d 0h 0m");
    assert_eq!(format_duration(90_061), "1d 1h 1m");
}

#[test]
fn plot_x_anchors_newest_at_right_when_full() {
    // Full queue: oldest sample at x=0, newest at x=HISTORY_LEN-1.
    assert_eq!(plot_x_for_sample(0, HISTORY_LEN), 0.0);
    assert_eq!(
        plot_x_for_sample(HISTORY_LEN - 1, HISTORY_LEN),
        (HISTORY_LEN - 1) as f64
    );
}

#[test]
fn plot_x_anchors_newest_at_right_when_partial() {
    // Half-full queue (30 samples): newest still lands at HISTORY_LEN-1,
    // oldest at HISTORY_LEN - data_len.
    let n = 30;
    assert_eq!(plot_x_for_sample(0, n), (HISTORY_LEN - n) as f64);
    assert_eq!(plot_x_for_sample(n - 1, n), (HISTORY_LEN - 1) as f64);
}

#[test]
fn sample_for_plot_x_inverse_of_plot_x_for_sample() {
    for n in [1, 15, 30, HISTORY_LEN] {
        for i in 0..n {
            let x = plot_x_for_sample(i, n);
            assert_eq!(sample_for_plot_x(x, n), Some(i), "n={n} i={i}");
        }
    }
}

#[test]
fn sample_for_plot_x_returns_none_outside_data() {
    // Left of the data region (when partially full) is empty space.
    assert_eq!(sample_for_plot_x(0.0, 10), None);
    // Right of HISTORY_LEN-1 is past the end.
    assert_eq!(sample_for_plot_x(HISTORY_LEN as f64, HISTORY_LEN), None);
    // Empty queue: nothing to hover.
    assert_eq!(sample_for_plot_x(5.0, 0), None);
}

#[test]
fn max_in_returns_zero_for_empty() {
    let empty: [f64; 0] = [];
    assert_eq!(max_in(empty.iter().copied()), 0.0);
}

#[test]
fn max_in_handles_negatives_by_collapsing_to_zero() {
    // Reductions seed with 0.0 — negative-only input still yields 0.
    assert_eq!(max_in([-5.0, -1.0, -100.0].iter().copied()), 0.0);
}

#[test]
fn max_in_returns_largest_positive() {
    assert_eq!(max_in([0.5, 3.0, 1.7, 2.9].iter().copied()), 3.0);
}
