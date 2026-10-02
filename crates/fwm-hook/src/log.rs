//! fwm.log in the data folder. Each line is flushed straight away so the log is
//! complete even if the game is killed.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static FILE: Mutex<Option<File>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();

/// Start a fresh log, keeping the previous run's as fwm.previous.log.
pub fn open(path: &Path) {
    START.get_or_init(Instant::now);
    if path.exists() {
        let _ = std::fs::rename(path, path.with_extension("previous.log"));
    }
    if let Ok(file) = File::create(path) {
        *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
    }
}

pub fn write(args: std::fmt::Arguments) {
    let seconds = START.get_or_init(Instant::now).elapsed().as_secs_f64();
    let line = format!("{seconds:9.3} {args}\n");
    if let Ok(mut guard) = FILE.lock() {
        if let Some(file) = guard.as_mut() {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}
