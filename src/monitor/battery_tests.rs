use super::*;

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

fn energy_raw() -> Raw {
    Raw {
        status: s("Discharging"),
        capacity: s("87"),
        energy_now: s("49000000"),         // 49 Wh
        energy_full: s("56000000"),        // 56 Wh
        energy_full_design: s("61000000"), // 61 Wh
        power_now: s("8400000"),           // 8.4 W
        cycle_count: s("123"),
        technology: s("Li-poly"),
        model_name: s("DELL XYZ"),
        ..Default::default()
    }
}

#[test]
fn builds_from_energy_set() {
    let b = build(&energy_raw(), false);
    assert_eq!(b.capacity_pct, 87.0);
    assert!(matches!(b.status, Status::Discharging));
    assert!((b.energy_now_wh - 49.0).abs() < 1e-3);
    assert!((b.energy_full_wh - 56.0).abs() < 1e-3);
    assert!((b.energy_full_design_wh - 61.0).abs() < 1e-3);
    assert!((b.power_w - 8.4).abs() < 1e-3);
    assert_eq!(b.cycle_count, 123);
    assert_eq!(b.technology, "Li-poly");
    assert_eq!(b.model, "DELL XYZ");
    assert!(!b.ac_online);
}

#[test]
fn falls_back_to_charge_set() {
    let raw = Raw {
        status: s("Charging"),
        charge_now: s("3000000"),          // 3 Ah
        charge_full: s("4000000"),         // 4 Ah
        charge_full_design: s("5000000"),  // 5 Ah
        voltage_min_design: s("11400000"), // 11.4 V
        current_now: s("2000000"),         // 2 A
        voltage_now: s("12000000"),        // 12 V
        ..Default::default()
    };
    let b = build(&raw, true);
    assert!((b.energy_now_wh - 34.2).abs() < 1e-3);
    assert!((b.energy_full_wh - 45.6).abs() < 1e-3);
    assert!((b.energy_full_design_wh - 57.0).abs() < 1e-3);
    // No power_now: derived from current × voltage.
    assert!((b.power_w - 24.0).abs() < 1e-3);
    // No capacity file: derived from energy_now / energy_full.
    assert!((b.capacity_pct - 75.0).abs() < 0.1);
    assert!(b.ac_online);
}

#[test]
fn negative_current_yields_positive_power() {
    let raw = Raw {
        current_now: s("-1500000"),
        voltage_now: s("10000000"),
        ..Default::default()
    };
    assert!((build(&raw, false).power_w - 15.0).abs() < 1e-3);
}

#[test]
fn unknown_or_missing_status() {
    assert!(matches!(
        build(&Raw::default(), false).status,
        Status::Unknown
    ));
    let raw = Raw {
        status: s("Not charging"),
        ..Default::default()
    };
    assert!(matches!(build(&raw, false).status, Status::NotCharging));
}

#[test]
fn missing_values_default_to_zero() {
    let b = build(&Raw::default(), false);
    assert_eq!(b.capacity_pct, 0.0);
    assert_eq!(b.power_w, 0.0);
    assert_eq!(b.energy_full_wh, 0.0);
    assert_eq!(b.cycle_count, 0);
    assert!(b.health_pct().is_none());
    assert!(b.time_left_secs().is_none());
}

#[test]
fn time_left_discharging_and_charging() {
    let mut b = build(&energy_raw(), false);
    // 49 Wh at 8.4 W → 5.83 h.
    assert_eq!(b.time_left_secs(), Some(21000));
    b.status = Status::Charging;
    // (56 − 49) Wh at 8.4 W → 50 min.
    assert_eq!(b.time_left_secs(), Some(3000));
    b.status = Status::Full;
    assert_eq!(b.time_left_secs(), None);
    b.status = Status::Discharging;
    b.power_w = 0.1; // below the extrapolation threshold
    assert_eq!(b.time_left_secs(), None);
}

#[test]
fn health_from_design_capacity() {
    let b = build(&energy_raw(), false);
    let h = b.health_pct().unwrap();
    assert!((h - 91.8).abs() < 0.1);
}
