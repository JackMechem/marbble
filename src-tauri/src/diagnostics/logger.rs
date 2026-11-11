//! The app's logger: env_logger to stderr as before (`RUST_LOG`, info by default), with every line
//! it lets through also kept in the [`LogRing`] the Diagnostics window reads. Installed once, at
//! startup (`startup::prepare_environment`).

use std::sync::OnceLock;
use std::time::Instant;

use super::log_ring::{LogRing, LINES};

/// The lines kept for the Diagnostics window.
pub fn ring() -> &'static LogRing {
    static RING: OnceLock<LogRing> = OnceLock::new();
    RING.get_or_init(|| LogRing::new(LINES))
}

/// When the logger started, which the lines' times count from.
fn started() -> Instant {
    static STARTED: OnceLock<Instant> = OnceLock::new();
    *STARTED.get_or_init(Instant::now)
}

struct Tee {
    stderr: env_logger::Logger,
}

impl log::Log for Tee {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        self.stderr.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        if !self.stderr.matches(record) {
            return;
        }
        self.stderr.log(record);
        ring().push(started().elapsed().as_millis() as u64, record.level(), record.target(), record.args().to_string());
    }

    fn flush(&self) {
        self.stderr.flush();
    }
}

/// Logs to stderr and the ring, filtered by `RUST_LOG` or else `default_filter`.
pub fn install(default_filter: &str) {
    started();
    let stderr = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_filter)).build();
    let max = stderr.filter();
    if log::set_boxed_logger(Box::new(Tee { stderr })).is_ok() {
        log::set_max_level(max);
    }
}
