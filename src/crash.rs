//! Crash reports (desktop). A panic writes a report next to the app's
//! settings and to the log; the next start shows it with a link to report
//! it. The work itself comes back from the kept session.

use std::path::{Path, PathBuf};

/// The report file's name in the app's data folder.
pub const FILE: &str = "crash-report.txt";
/// Where problems are reported.
pub const ISSUES: &str = "https://github.com/ash-hashtag/shine/issues/new";

/// A crash report: what happened, where, on which version and system.
pub fn report(message: &str, location: &str, backtrace: &str) -> String {
    format!(
        "Shine {} crashed.\n\nMessage: {message}\nAt: {location}\nSystem: {} {}\n\nBacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Write a report to `dir` (and the log) whenever the app panics, then let
/// the default handling continue.
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let location = info.location().map_or_else(|| "unknown".into(), |l| format!("{}:{}", l.file(), l.line()));
        let text = report(&message, &location, &std::backtrace::Backtrace::force_capture().to_string());
        tracing::error!("{text}");
        let _ = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(FILE), &text));
        previous(info);
    }));
}

/// The report left by a crash in the last run, if any. It's moved aside so
/// it's shown once.
pub fn take_last(dir: &Path) -> Option<String> {
    let path = dir.join(FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::rename(&path, dir.join("crash-report.previous.txt"));
    Some(text)
}

/// A link that opens a new GitHub issue with the report filled in.
pub fn issue_url(report: &str) -> String {
    let title = report.lines().find_map(|l| l.strip_prefix("Message: ")).unwrap_or("Crash");
    // Links have a practical length limit; the start of the backtrace is enough.
    let body: String = report.chars().take(4000).collect();
    format!("{ISSUES}?title={}&body={}", encode(&format!("Crash: {title}")), encode(&format!("```\n{body}\n```")))
}

/// Percent-encoding for a URL query value.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_say_what_where_and_which_version() {
        let r = report("index out of bounds", "src/x.rs:10", "0: main");
        assert!(r.starts_with(&format!("Shine {} crashed.", env!("CARGO_PKG_VERSION"))));
        assert!(r.contains("Message: index out of bounds") && r.contains("At: src/x.rs:10") && r.contains("0: main"));
        assert!(r.contains(std::env::consts::OS));
    }

    #[test]
    fn issue_links_are_encoded_and_bounded() {
        let r = report("a & b = c?", "src/x.rs:1", &"frame\n".repeat(5000));
        let url = issue_url(&r);
        assert!(url.starts_with(ISSUES));
        assert!(url.contains("title=Crash%3A%20a%20%26%20b%20%3D%20c%3F"), "{}", &url[..200]);
        assert!(url.len() < 16_000, "{}", url.len());
        assert!(!url.contains(' ') && !url.contains('\n'));
    }

    #[test]
    fn the_last_report_is_shown_once() {
        let dir = std::env::temp_dir().join(format!("shine-crash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(take_last(&dir), None);
        std::fs::write(dir.join(FILE), "boom").unwrap();
        assert_eq!(take_last(&dir).as_deref(), Some("boom"));
        assert_eq!(take_last(&dir), None, "moved aside");
        std::fs::remove_dir_all(&dir).ok();
    }
}
