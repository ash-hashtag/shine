//! Logging setup: human-readable logs on stderr, plus an optional log file.
//!
//! Configuration, highest precedence first:
//! - filter: `--log-level <filter>`, `SHINE_LOG`, `RUST_LOG`, default [`DEFAULT_FILTER`];
//! - file: `--log-file <path>` or `SHINE_LOG_FILE` (appends; parent dirs are created).
//!
//! Filters use `tracing_subscriber::EnvFilter` syntax, e.g. `debug` or
//! `info,shine::photonics=trace`.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;

use tracing::Subscriber;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

/// Our own logs at `info`, noisy windowing/graphics crates at `warn`.
pub const DEFAULT_FILTER: &str =
    "info,wgpu=warn,wgpu_core=warn,wgpu_hal=warn,naga=warn,eframe=warn,egui_winit=warn,winit=warn";

#[derive(Clone, Debug, PartialEq)]
pub struct LogConfig {
    pub filter: String,
    pub file: Option<PathBuf>,
    /// Colour codes on stderr.
    pub ansi: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            filter: DEFAULT_FILTER.to_owned(),
            file: None,
            ansi: std::io::stderr().is_terminal(),
        }
    }
}

impl LogConfig {
    /// Read the process arguments and environment.
    pub fn from_env() -> Self {
        Self::from_sources(std::env::args_os().skip(1), |k| std::env::var(k).ok())
    }

    /// Build from explicit arguments (without the program name) and an
    /// environment lookup. Unrelated arguments are ignored.
    pub fn from_sources(
        args: impl IntoIterator<Item = OsString>,
        env: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let mut config = Self::default();
        let mut arg_filter = None;
        let mut arg_file = None;

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let arg = arg.to_string_lossy().into_owned();
            let (key, inline) = match arg.split_once('=') {
                Some((k, v)) => (k.to_owned(), Some(v.to_owned())),
                None => (arg, None),
            };
            let mut value = || {
                inline
                    .clone()
                    .or_else(|| args.next().map(|v| v.to_string_lossy().into_owned()))
            };
            match key.as_str() {
                "--log-level" => arg_filter = value(),
                "--log-file" => arg_file = value(),
                _ => {}
            }
        }

        let non_empty = |v: Option<String>| v.filter(|s| !s.trim().is_empty());
        if let Some(filter) = non_empty(arg_filter)
            .or_else(|| non_empty(env("SHINE_LOG")))
            .or_else(|| non_empty(env("RUST_LOG")))
        {
            config.filter = filter;
        }
        config.file = non_empty(arg_file)
            .or_else(|| non_empty(env("SHINE_LOG_FILE")))
            .map(PathBuf::from);
        config
    }
}

/// Keeps the background file writer alive; drop it at exit to flush.
#[must_use = "dropping the guard stops file logging"]
pub struct LogGuard {
    _file: Option<WorkerGuard>,
}

#[derive(Debug)]
pub enum LogError {
    Filter(String),
    File(std::io::Error),
    AlreadyInitialised,
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Filter(e) => write!(f, "invalid log filter: {e}"),
            Self::File(e) => write!(f, "cannot open log file: {e}"),
            Self::AlreadyInitialised => write!(f, "a global logger is already installed"),
        }
    }
}

impl std::error::Error for LogError {}

/// Build the subscriber without installing it.
pub fn subscriber(
    config: &LogConfig,
) -> Result<(impl Subscriber + Send + Sync + use<>, LogGuard), LogError> {
    let filter = EnvFilter::try_new(&config.filter).map_err(|e| LogError::Filter(e.to_string()))?;

    let stderr = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(config.ansi)
        .with_target(true);

    let (file_layer, guard) = match &config.file {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(LogError::File)?;
            }
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(LogError::File)?;
            let (writer, guard) = tracing_appender::non_blocking(file);
            let layer = fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(true)
                .with_thread_ids(true);
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    let subscriber =
        tracing_subscriber::registry().with(stderr.and_then(file_layer).with_filter(filter));
    Ok((subscriber, LogGuard { _file: guard }))
}

/// Install the global logger. Call once, early in `main`, and keep the guard.
pub fn init(config: &LogConfig) -> Result<LogGuard, LogError> {
    let (subscriber, guard) = subscriber(config)?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| LogError::AlreadyInitialised)?;
    if let Some(file) = &config.file {
        tracing::info!("Logging to {}", file.display());
    }
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("shine-logging-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_without_args_or_env() {
        let c = LogConfig::from_sources(args(&[]), no_env);
        assert_eq!(c.filter, DEFAULT_FILTER);
        assert_eq!(c.file, None);
    }

    #[test]
    fn parses_both_argument_styles() {
        let c = LogConfig::from_sources(
            args(&["--other", "--log-level", "debug", "--log-file=/tmp/a.log"]),
            no_env,
        );
        assert_eq!(c.filter, "debug");
        assert_eq!(c.file, Some(PathBuf::from("/tmp/a.log")));
        let c =
            LogConfig::from_sources(args(&["--log-level=trace", "--log-file", "b.log"]), no_env);
        assert_eq!(
            (c.filter.as_str(), c.file),
            ("trace", Some(PathBuf::from("b.log")))
        );
    }

    #[test]
    fn missing_or_empty_values_are_ignored() {
        let c = LogConfig::from_sources(args(&["--log-file"]), no_env);
        assert_eq!(c.file, None);
        let c = LogConfig::from_sources(args(&["--log-level="]), no_env);
        assert_eq!(c.filter, DEFAULT_FILTER);
    }

    #[test]
    fn precedence_args_then_shine_then_rust_log() {
        let env = |k: &str| match k {
            "SHINE_LOG" => Some("warn".to_owned()),
            "RUST_LOG" => Some("error".to_owned()),
            "SHINE_LOG_FILE" => Some("env.log".to_owned()),
            _ => None,
        };
        let c = LogConfig::from_sources(args(&[]), env);
        assert_eq!(
            (c.filter.as_str(), c.file),
            ("warn", Some(PathBuf::from("env.log")))
        );
        let c = LogConfig::from_sources(
            args(&["--log-level", "debug", "--log-file", "arg.log"]),
            env,
        );
        assert_eq!(
            (c.filter.as_str(), c.file),
            ("debug", Some(PathBuf::from("arg.log")))
        );
        let rust_only = |k: &str| (k == "RUST_LOG").then(|| "error".to_owned());
        assert_eq!(
            LogConfig::from_sources(args(&[]), rust_only).filter,
            "error"
        );
    }

    #[test]
    fn rejects_bad_filter() {
        let config = LogConfig {
            filter: "[[[".into(),
            ..LogConfig::default()
        };
        let err = subscriber(&config).err().unwrap();
        assert!(err.to_string().starts_with("invalid log filter"));
    }

    #[test]
    fn writes_filtered_events_to_file() {
        let dir = scratch_dir("writes");
        let path = dir.join("nested").join("shine.log");
        let config = LogConfig {
            filter: "info".into(),
            file: Some(path.clone()),
            ansi: false,
        };
        let (sub, guard) = subscriber(&config).unwrap();
        tracing::subscriber::with_default(sub, || {
            tracing::info!(target: "shine::test", "hello file");
            tracing::debug!("filtered out");
        });
        drop(guard);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("hello file") && text.contains("shine::test"),
            "{text}"
        );
        assert!(!text.contains("filtered out"));
        assert!(!text.contains('\u{1b}'), "no colour codes in files");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn appends_to_existing_file() {
        let dir = scratch_dir("appends");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shine.log");
        std::fs::write(&path, "previous run\n").unwrap();
        let config = LogConfig {
            filter: "info".into(),
            file: Some(path.clone()),
            ansi: false,
        };
        let (sub, guard) = subscriber(&config).unwrap();
        tracing::subscriber::with_default(sub, || tracing::warn!("second run"));
        drop(guard);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("previous run\n") && text.contains("second run"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unwritable_file_is_an_error() {
        let dir = scratch_dir("unwritable");
        std::fs::create_dir_all(&dir).unwrap();
        // A directory cannot be opened as a log file.
        let config = LogConfig {
            file: Some(dir.clone()),
            ..LogConfig::default()
        };
        let err = subscriber(&config).err().unwrap();
        assert!(matches!(err, LogError::File(_)), "{err}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_display() {
        assert_eq!(
            LogError::AlreadyInitialised.to_string(),
            "a global logger is already installed"
        );
    }
}
