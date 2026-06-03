use super::*;

#[test]
fn url_encode_passes_through_unreserved() {
    // RFC 3986 unreserved set: A-Z a-z 0-9 - _ . ~
    assert_eq!(url_encode("Hello-World_1.2~3"), "Hello-World_1.2~3");
}

#[test]
fn url_encode_spaces_become_plus() {
    // We're building a Google query URL — spaces are application/x-www-form-urlencoded.
    assert_eq!(url_encode("linux process firefox"), "linux+process+firefox");
}

#[test]
fn url_encode_special_chars_become_percent_hex() {
    assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(url_encode("?#/"), "%3F%23%2F");
}

#[test]
fn url_encode_non_ascii_byte_wise() {
    // UTF-8 byte for é (0xC3 0xA9) → "%C3%A9"
    assert_eq!(url_encode("é"), "%C3%A9");
}
