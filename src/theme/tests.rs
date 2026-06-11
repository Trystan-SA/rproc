use super::*;

#[test]
fn parses_single_line_variant_reply() {
    let reply = "method return time=1781194240.689573 sender=:1.28 -> destination=:1.529 serial=5464 reply_serial=2\n   variant       variant          uint32 1\n";
    assert_eq!(parse_dbus_color_scheme(reply), Some(Theme::Dark));
}

#[test]
fn parses_light_preference() {
    let reply = "method return time=1.0 sender=:1.28 -> destination=:1.529 serial=1 reply_serial=2\n   variant       variant          uint32 2\n";
    assert_eq!(parse_dbus_color_scheme(reply), Some(Theme::Light));
}

#[test]
fn parses_no_preference_as_light() {
    let reply = "   variant       variant          uint32 0\n";
    assert_eq!(parse_dbus_color_scheme(reply), Some(Theme::Light));
}

#[test]
fn parses_multi_line_variant_reply() {
    let reply = "method return time=1.0 sender=:1.42 -> destination=:1.99 serial=7 reply_serial=2\n   variant\n      variant\n         uint32 1\n";
    assert_eq!(parse_dbus_color_scheme(reply), Some(Theme::Dark));
}

#[test]
fn rejects_reply_without_uint32() {
    assert_eq!(
        parse_dbus_color_scheme("method return\n   variant string \"x\"\n"),
        None
    );
    assert_eq!(parse_dbus_color_scheme(""), None);
}

#[test]
fn rejects_unparsable_value() {
    assert_eq!(parse_dbus_color_scheme("variant uint32 oops"), None);
    assert_eq!(parse_dbus_color_scheme("variant uint32"), None);
}
