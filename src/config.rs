//! Filesystem locations and small shared helpers. machineline keeps two things on disk:
//!
//!   * a config dir (`~/.config/machineline/`) — the captured "base" status-line block we
//!     delegate to (see `install.rs`);
//!   * a state dir (`~/.claude/machineline/`) — the cached metrics snapshot (see `cache.rs`),
//!     which also carries the previous CPU tick counters the utilisation delta is computed
//!     against.
//!
//! Both can be relocated with env vars so tests and odd setups never touch the real ones.

use std::env;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Config dir: `$MACHINELINE_CONFIG_DIR`, else `~/.config/machineline`.
pub fn config_dir() -> PathBuf {
    if let Some(d) = env::var_os("MACHINELINE_CONFIG_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    match home_dir() {
        Some(h) => h.join(".config").join("machineline"),
        None => PathBuf::from(".config-machineline"),
    }
}

/// State/cache dir: `$MACHINELINE_STATE_DIR`, else `~/.claude/machineline` (next to the others).
pub fn state_dir() -> PathBuf {
    if let Some(d) = env::var_os("MACHINELINE_STATE_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    match home_dir() {
        Some(h) => h.join(".claude").join("machineline"),
        None => PathBuf::from(".state-machineline"),
    }
}

/// Current Unix time in seconds (fractional). 0 if the clock predates the epoch.
pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
