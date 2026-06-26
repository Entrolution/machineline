//! Tiny helper for the metric modules that read a stock macOS CLI (`pmset`, `vm_stat`, `ps`,
//! `sysctl`). Spawning these only ever happens in `machineline refresh` — the background path —
//! never during a render, so the prompt can't stall on a process spawn.

use std::process::Command;

/// Run `cmd args…` and return its stdout as a String, or `None` on spawn failure / non-zero exit.
pub fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}
