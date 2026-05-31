use super::*;

#[test]
fn serialize_cache_round_trips_positive_and_negative_entries() {
    // The on-disk line format mirrors the in-memory key (`name\0exe`); a
    // positive entry keeps its uri, a negative one has an empty third field.
    let mut cache = HashMap::new();
    cache.insert(
        "firefox\u{0}/usr/bin/firefox".to_string(),
        Some("file:///icons/firefox.png".to_string()),
    );
    cache.insert("kworker\u{0}".to_string(), None);

    let text = serialize_cache(&cache);

    // Re-parse with the same split logic load_persistent uses.
    let mut seen = HashMap::new();
    for line in text.lines() {
        let (key, uri) = line.split_once('\t').unwrap();
        seen.insert(key.to_string(), uri.to_string());
    }
    assert_eq!(
        seen.get("firefox\u{0}/usr/bin/firefox").map(String::as_str),
        Some("file:///icons/firefox.png")
    );
    assert_eq!(seen.get("kworker\u{0}").map(String::as_str), Some(""));
}

#[test]
fn serialize_cache_skips_entries_with_tab_or_newline() {
    // A tab/newline in the key would split into a bogus extra field on load,
    // so such entries must never be written.
    let mut cache = HashMap::new();
    cache.insert(
        "bad\tkey\u{0}".to_string(),
        Some("file:///x.png".to_string()),
    );
    cache.insert("bad\nkey\u{0}".to_string(), None);
    cache.insert("ok\u{0}".to_string(), Some("file:///ok.png".to_string()));

    let text = serialize_cache(&cache);

    assert_eq!(text.lines().count(), 1);
    assert!(text.starts_with("ok\u{0}\tfile:///ok.png"));
}
