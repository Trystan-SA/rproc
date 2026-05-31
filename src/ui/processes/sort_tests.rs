use super::*;

#[test]
fn sort_key_roundtrip_all_variants() {
    // Every variant must roundtrip through the on-disk encoding,
    // otherwise saving today's sort silently resets to the default
    // tomorrow.
    for k in [
        SortKey::Name,
        SortKey::Pid,
        SortKey::User,
        SortKey::Cpu,
        SortKey::Mem,
        SortKey::Disk,
        SortKey::Status,
    ] {
        let s = k.as_str();
        assert_eq!(SortKey::from_str(s), Some(k), "roundtrip for {s}");
    }
}

#[test]
fn sort_key_from_str_rejects_unknown() {
    assert_eq!(SortKey::from_str(""), None);
    assert_eq!(SortKey::from_str("not-a-key"), None);
    // Case-sensitive — we control both sides of the format.
    assert_eq!(SortKey::from_str("cpu"), None);
}

#[test]
fn sort_prefs_roundtrip_via_file() {
    let dir = std::env::temp_dir().join(format!(
        "rproc-sort-prefs-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("prefs.txt");
    save_sort_prefs_to(&path, SortKey::Mem, false).unwrap();
    let loaded = load_sort_prefs_from(&path);
    assert_eq!(loaded, Some((SortKey::Mem, false)));

    save_sort_prefs_to(&path, SortKey::Cpu, true).unwrap();
    assert_eq!(load_sort_prefs_from(&path), Some((SortKey::Cpu, true)));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sort_prefs_missing_file_returns_none() {
    let bogus = std::path::Path::new("/nonexistent/rproc/prefs.txt.does-not-exist");
    assert_eq!(load_sort_prefs_from(bogus), None);
}

#[test]
fn sort_prefs_partial_file_returns_none() {
    // We refuse to apply a half-saved file rather than silently
    // defaulting one half — the user would never notice.
    let dir = std::env::temp_dir().join(format!(
        "rproc-sort-partial-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("partial.txt");
    std::fs::write(&path, "sort=Cpu\n").unwrap();
    assert_eq!(load_sort_prefs_from(&path), None);

    std::fs::write(&path, "descending=true\n").unwrap();
    assert_eq!(load_sort_prefs_from(&path), None);

    let _ = std::fs::remove_dir_all(&dir);
}
