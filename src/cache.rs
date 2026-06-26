//! The on-disk snapshot that keeps process spawns off the render path.
//!
//! `render` reads `state.json` and prints from it *immediately*, then — if the snapshot is older
//! than `TTL` — spawns a detached `machineline refresh` to update it for next time. So the prompt
//! never blocks on `pmset`/`vm_stat`/`ps`; the displayed numbers are at most `TTL + one render
//! tick` stale, which for an at-a-glance machine readout is fine.
//!
//! The snapshot also carries the previous CPU tick counters, so each refresh computes utilisation
//! as the delta since the last one — no in-render sleep to take a sample. A short-lived lock file
//! stops a burst of render ticks from spawning a pile of overlapping refreshes.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::config::{now_secs, state_dir};

/// How long a snapshot is considered fresh. Machine metrics move fast, so this is short — but the
/// background refresh means a short TTL costs a cheap spawn, never prompt latency.
pub const TTL_SECS: f64 = 4.0;

/// Past this age the reading is flagged `(stale)` — shown (better than nothing) but not mistaken
/// for live. Reached only when no Claude render has fired for a while.
pub const MAX_AGE_SECS: f64 = 120.0;

/// Reclaim a lock older than this — covers a refresh child that died before clearing it.
const LOCK_GRACE_SECS: f64 = 10.0;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct State {
    /// Unix seconds of the last successful gather.
    pub fetched_at: f64,
    #[serde(default)]
    pub last_attempt: f64,

    // ---- CPU ----
    #[serde(default)]
    pub speed_limit: Option<u32>, // throttle %, 100 = full speed
    #[serde(default)]
    pub load1: Option<f64>,
    #[serde(default)]
    pub ncpu: u32,
    #[serde(default)]
    pub cpu_util: Option<f64>,
    /// Cumulative tick counters carried for the next delta — not displayed.
    #[serde(default)]
    pub cpu_busy_ticks: Option<u64>,
    #[serde(default)]
    pub cpu_idle_ticks: Option<u64>,

    // ---- memory ----
    #[serde(default)]
    pub mem_used_pct: Option<f64>,
    #[serde(default)]
    pub mem_pressure: Option<u8>, // 1 normal, 2 warn, 4 critical
    #[serde(default)]
    pub swap_used: Option<f64>, // bytes

    // ---- hottest process ----
    #[serde(default)]
    pub top_name: Option<String>,
    #[serde(default)]
    pub top_cpu: Option<f64>,

    // ---- thermal ----
    #[serde(default)]
    pub temp_c: Option<f64>,

    // ---- power ----
    #[serde(default)]
    pub charge_pct: Option<f64>,
    #[serde(default)]
    pub on_ac: Option<bool>,
    #[serde(default)]
    pub charging: Option<bool>,
    #[serde(default)]
    pub draining_on_ac: bool,
    #[serde(default)]
    pub adapter_w: Option<u32>,
}

impl State {
    pub fn age(&self, now: f64) -> f64 {
        (now - self.fetched_at).max(0.0)
    }

    pub fn refresh_due(&self, now: f64) -> bool {
        (now - self.last_attempt).max(0.0) > TTL_SECS
    }

    pub fn is_expired(&self, now: f64) -> bool {
        self.age(now) > MAX_AGE_SECS
    }
}

/// Gather a fresh snapshot. `prev` (the last snapshot) supplies the CPU tick baseline so this
/// reading can report utilisation as the delta since then.
pub fn gather(prev: Option<&State>, now: f64) -> State {
    let cur_ticks = crate::cpu::ticks();
    let cpu_util = match (prev, cur_ticks) {
        (Some(p), Some(cur)) => match (p.cpu_busy_ticks, p.cpu_idle_ticks) {
            (Some(busy), Some(idle)) => {
                crate::cpu::utilization(crate::cpu::Ticks { busy, idle }, cur)
            }
            _ => None,
        },
        _ => None,
    };

    let mem = crate::mem::read();
    let power = crate::power::read();
    let top = crate::proc::top();

    State {
        fetched_at: now,
        last_attempt: now,
        speed_limit: crate::throttle::speed_limit(),
        load1: crate::loadavg::load1(),
        ncpu: crate::loadavg::ncpu() as u32,
        cpu_util,
        cpu_busy_ticks: cur_ticks.map(|t| t.busy),
        cpu_idle_ticks: cur_ticks.map(|t| t.idle),
        mem_used_pct: mem.used_pct,
        mem_pressure: mem.pressure,
        swap_used: mem.swap_used,
        top_name: top.as_ref().map(|t| t.name.clone()),
        top_cpu: top.as_ref().map(|t| t.cpu),
        temp_c: crate::temp::cpu_celsius(),
        charge_pct: power.charge_pct,
        on_ac: power.on_ac,
        charging: power.charging,
        draining_on_ac: power.draining_on_ac,
        adapter_w: power.adapter_w,
    }
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join("state.json")
}

fn lock_path(dir: &Path) -> PathBuf {
    dir.join("refresh.lock")
}

/// Read the cached snapshot, or `None` if absent/unreadable/corrupt.
pub fn read(dir: &Path) -> Option<State> {
    let text = fs::read_to_string(state_path(dir)).ok()?;
    serde_json::from_str::<State>(&text).ok()
}

/// Atomically persist a snapshot (per-process temp + rename, like vastline's writer).
pub fn write(dir: &Path, state: &State) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(state).unwrap_or_default();
    let tmp = dir.join(format!("state.{}.json.tmp", std::process::id()));
    fs::write(&tmp, json)?;
    if let Err(e) = fs::rename(&tmp, state_path(dir)) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn lock_age(dir: &Path, now: f64) -> Option<f64> {
    let mtime = fs::metadata(lock_path(dir))
        .and_then(|m| m.modified())
        .ok()?;
    let secs = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs_f64();
    Some((now - secs).max(0.0))
}

/// Acquire the refresh lock atomically (O_EXCL). Only the single creator wins; a stale lock from a
/// crashed refresh is reclaimed after `LOCK_GRACE_SECS`.
fn acquire_lock(dir: &Path, now: f64) -> bool {
    let _ = fs::create_dir_all(dir);
    if let Some(age) = lock_age(dir, now) {
        if age >= LOCK_GRACE_SECS {
            let _ = fs::remove_file(lock_path(dir));
        }
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_path(dir))
    {
        Ok(mut f) => {
            let _ = write!(f, "{now}");
            true
        }
        Err(_) => false,
    }
}

/// Spawn a detached `machineline refresh` if a refresh is due and we win the lock. Best-effort and
/// non-blocking: a failure just means we try again next render tick.
pub fn spawn_refresh_if_stale(dir: &Path, state: Option<&State>, now: f64) {
    let due = match state {
        None => true,
        Some(s) => s.refresh_due(now),
    };
    if !due {
        return;
    }
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    if !acquire_lock(dir, now) {
        return;
    }
    if Command::new(exe)
        .arg("refresh")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_err()
    {
        let _ = fs::remove_file(lock_path(dir));
    }
}

/// `machineline refresh` — gather metrics and rewrite the cache. Synchronous by design: it is what
/// the detached child (or a manual run) executes. Always clears the lock on the way out.
pub fn run_refresh() -> i32 {
    let dir = state_dir();
    let now = now_secs();
    let _ = fs::create_dir_all(&dir);
    let _ = fs::write(lock_path(&dir), format!("{now}"));

    let prev = read(&dir);
    let state = gather(prev.as_ref(), now);
    let code = match write(&dir, &state) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("machineline: could not write cache: {e}");
            1
        }
    };

    let _ = fs::remove_file(lock_path(&dir));
    code
}
