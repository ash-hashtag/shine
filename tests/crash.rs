//! The panic hook writes a crash report (in its own process: hooks are global).

#[test]
fn a_panic_leaves_a_report() {
    let dir = std::env::temp_dir().join(format!("shine-crash-hook-{}", std::process::id()));
    shine::crash::install(dir.clone());
    let result = std::panic::catch_unwind(|| panic!("simulated failure {}", 42));
    assert!(result.is_err());
    let text = std::fs::read_to_string(dir.join(shine::crash::FILE)).expect("a report was written");
    assert!(text.contains("Message: simulated failure 42"), "{text}");
    assert!(text.contains("tests/crash.rs"), "where it happened: {text}");
    assert_eq!(shine::crash::take_last(&dir).as_deref(), Some(text.as_str()));
    std::fs::remove_dir_all(&dir).ok();
}
