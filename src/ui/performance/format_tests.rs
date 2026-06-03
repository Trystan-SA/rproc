use super::*;
use crate::monitor::system::NetInfo;

fn net(name: &str) -> NetInfo {
    NetInfo {
        name: name.to_string(),
        ..Default::default()
    }
}

#[test]
fn iface_kind_classifies_by_prefix() {
    assert_eq!(iface_kind("wlan0"), "Wi-Fi");
    assert_eq!(iface_kind("wlp3s0"), "Wi-Fi");
    assert_eq!(iface_kind("wwan0"), "Mobile broadband");
    assert_eq!(iface_kind("usb0"), "USB tethering");
    assert_eq!(iface_kind("rndis0"), "USB tethering");
    assert_eq!(iface_kind("enp0s3"), "Ethernet");
    assert_eq!(iface_kind("eth0"), "Ethernet");
    assert_eq!(iface_kind("lo"), "Network");
}

#[test]
fn iface_label_unqualified_when_kind_is_unique() {
    let nets = [net("enp0s3"), net("wlan0")];
    // One of each kind → no numeric suffix.
    assert_eq!(iface_label(&nets, 0), "Ethernet");
    assert_eq!(iface_label(&nets, 1), "Wi-Fi");
}

#[test]
fn iface_label_ranks_interfaces_of_the_same_kind() {
    let nets = [net("eth0"), net("wlan0"), net("eth1")];
    // Two Ethernet interfaces → ranked 1 and 2 by their order.
    assert_eq!(iface_label(&nets, 0), "Ethernet 1");
    assert_eq!(iface_label(&nets, 2), "Ethernet 2");
    // The lone Wi-Fi stays unqualified.
    assert_eq!(iface_label(&nets, 1), "Wi-Fi");
}

#[test]
fn iface_label_out_of_range_is_generic() {
    let nets = [net("eth0")];
    assert_eq!(iface_label(&nets, 5), "Network");
}

#[test]
fn short_disk_name_strips_dev_prefix() {
    assert_eq!(short_disk_name("/dev/nvme0n1"), "nvme0n1");
    // No prefix → unchanged.
    assert_eq!(short_disk_name("sda"), "sda");
}

#[test]
fn combined_disk_sums_elementwise_over_the_longer_len() {
    let a: VecDeque<f64> = [1.0, 2.0, 3.0].into();
    let b: VecDeque<f64> = [10.0, 20.0].into();
    // Missing tail elements count as 0.
    assert_eq!(combined_disk(&a, &b), VecDeque::from([11.0, 22.0, 3.0]));
}

#[test]
fn combined_disk_empty_inputs_yield_empty() {
    let empty: VecDeque<f64> = VecDeque::new();
    assert!(combined_disk(&empty, &empty).is_empty());
}

#[test]
fn temp_label_hides_unavailable_sentinel_and_nonfinite() {
    // 0.0 is the "no sensor" sentinel; NaN/inf also yield nothing.
    assert_eq!(temp_label(0.0), None);
    assert_eq!(temp_label(f32::NAN), None);
    assert_eq!(temp_label(f32::INFINITY), None);
    assert_eq!(temp_label(47.0), Some("47C".to_string()));
}
