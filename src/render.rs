//! Builds the single machine line from the cached snapshot, then (if configured) prints it *above*
//! the output of the base status-line command we delegate to — the same stacking trick vastline
//! uses to sit on top of quotaline. Installed as machineline → vastline → quotaline, each tool
//! prints its own line.
//!
//! Example line:
//!   mach  clk 100% · load 0.5 · cpu 12% · mem 38% · 41°C · top WindowServer 6% · ac 96W
//! Throttled (a hot day):
//!   mach  clk 31% · load 6.5 · cpu 100% · mem 71% · swap 2.4G · 96°C · top iTerm2 61% · ac⚠ 96W

use std::io::Read;
use std::process::{Command, Stdio};

use crate::cache::{read as read_cache, spawn_refresh_if_stale, State};
use crate::config::{now_secs, state_dir};
use crate::fmt::{
    band_throttle, band_up, fmt_bytes, fmt_load, AMBER, DIM, GRAY, GREEN, RED, RESET,
};
use crate::install::base_command;

const LABEL: &str = "mach";

/// Entry point for the default (no-arg) invocation: print the base line(s), then the machine line.
pub fn run_statusline() -> i32 {
    // Claude Code pipes a JSON session payload on stdin. machineline doesn't need it, but the base
    // command (vastline/quotaline) does — capture it once and forward it verbatim.
    let mut stdin_payload = String::new();
    let _ = std::io::stdin().read_to_string(&mut stdin_payload);

    let mut out = String::new();
    if let Some(base) = base_command() {
        if let Some(base_out) = run_base(&base, &stdin_payload) {
            out.push_str(&base_out);
            if !base_out.ends_with('\n') {
                out.push('\n');
            }
        }
    }

    let now = now_secs();
    let dir = state_dir();
    let state = read_cache(&dir);
    // Kick a background refresh for next time if the snapshot is stale; never blocks this render.
    spawn_refresh_if_stale(&dir, state.as_ref(), now);

    out.push_str(&line(state.as_ref(), now));
    out.push('\n');

    use std::io::Write;
    let _ = std::io::stdout().write_all(out.as_bytes());
    0
}

/// Run the captured base status-line command, feeding it the same stdin Claude Code gave us.
fn run_base(command: &str, stdin_payload: &str) -> Option<String> {
    // Loop-breaker: never delegate to ourselves (a poisoned base.json pointing back at machineline
    // would recurse without bound).
    if crate::install::looks_like_self(command) {
        return None;
    }
    let mut child = shell(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Feed stdin on a separate thread so a base command that fills its stdout pipe before draining
    // stdin can't deadlock us.
    if let Some(mut sin) = child.stdin.take() {
        use std::io::Write;
        let payload = stdin_payload.to_owned();
        std::thread::spawn(move || {
            let _ = sin.write_all(payload.as_bytes());
        });
    }
    let out = child.wait_with_output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(unix)]
fn shell(command: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(command);
    c
}

#[cfg(not(unix))]
fn shell(command: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(command);
    c
}

/// Render just the machine line (no base prefix). Pure given a snapshot + clock, for testing.
pub fn line(state: Option<&State>, now: f64) -> String {
    let label = format!("{DIM}{LABEL}{RESET}");
    let Some(s) = state else {
        return format!("{label}  {GRAY}gathering…{RESET}");
    };

    let mut segs: Vec<String> = Vec::new();
    if let Some(seg) = throttle_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = load_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = util_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = mem_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = swap_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = temp_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = top_seg(s) {
        segs.push(seg);
    }
    if let Some(seg) = power_seg(s) {
        segs.push(seg);
    }

    if segs.is_empty() {
        return format!("{label}  {GRAY}n/a{RESET}");
    }

    let mut out = format!("{label}  {}", segs.join(&format!("{DIM} · {RESET}")));
    if s.is_expired(now) {
        out.push_str(&format!("  {GRAY}(stale){RESET}"));
    }
    out
}

fn throttle_seg(s: &State) -> Option<String> {
    let lim = s.speed_limit?;
    Some(format!(
        "{DIM}clk{RESET} {}{lim}%{RESET}",
        band_throttle(lim)
    ))
}

fn load_seg(s: &State) -> Option<String> {
    let load = s.load1?;
    let ratio = if s.ncpu > 0 {
        load / s.ncpu as f64
    } else {
        load
    };
    Some(format!(
        "{DIM}load{RESET} {}{}{RESET}",
        band_up(ratio, 0.7, 1.0),
        fmt_load(load)
    ))
}

fn util_seg(s: &State) -> Option<String> {
    let u = s.cpu_util?;
    Some(format!(
        "{DIM}cpu{RESET} {}{}%{RESET}",
        band_up(u, 80.0, 95.0),
        u.round() as i64
    ))
}

fn mem_seg(s: &State) -> Option<String> {
    let used = s.mem_used_pct?;
    let color = match s.mem_pressure {
        Some(p) if p >= 4 => RED,
        Some(p) if p >= 2 => AMBER,
        Some(_) => GREEN,
        None => band_up(used, 75.0, 90.0),
    };
    Some(format!(
        "{DIM}mem{RESET} {color}{}%{RESET}",
        used.round() as i64
    ))
}

fn swap_seg(s: &State) -> Option<String> {
    let b = s.swap_used?;
    // Healthy default is zero swap — omit the segment unless something is actually swapped out.
    if b < 1.0 * 1024.0 * 1024.0 {
        return None;
    }
    let color = band_up(b, 256.0 * 1024.0 * 1024.0, 2.0 * 1024.0 * 1024.0 * 1024.0);
    Some(format!("{DIM}swap{RESET} {color}{}{RESET}", fmt_bytes(b)))
}

fn temp_seg(s: &State) -> Option<String> {
    let t = s.temp_c?;
    Some(format!(
        "{}{}°C{RESET}",
        band_up(t, 75.0, 90.0),
        t.round() as i64
    ))
}

fn top_seg(s: &State) -> Option<String> {
    let name = s.top_name.as_deref()?;
    let cpu = s.top_cpu?;
    Some(format!(
        "{DIM}top{RESET} {} {}{}%{RESET}",
        shorten(name, 18),
        band_up(cpu, 50.0, 90.0),
        cpu.round() as i64
    ))
}

/// Trim a long process name for the line: prefer the trailing component of a reverse-DNS bundle id
/// (`com.apple.WebKit.WebContent` → `WebContent`), then hard-cap the length with an ellipsis.
fn shorten(name: &str, max: usize) -> String {
    let base = if !name.contains(' ') && name.matches('.').count() >= 2 {
        name.rsplit('.').next().unwrap_or(name)
    } else {
        name
    };
    if base.chars().count() > max {
        let t: String = base.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    } else {
        base.to_string()
    }
}

fn power_seg(s: &State) -> Option<String> {
    let on_ac = s.on_ac?;
    if on_ac {
        if s.draining_on_ac {
            // The under-powered-adapter trap: on the wall but still draining the battery.
            let mut t = format!("{RED}ac⚠ drain{RESET}");
            if let Some(w) = s.adapter_w {
                t.push_str(&format!(" {RED}{w}W{RESET}"));
            }
            return Some(t);
        }
        let mut t = format!("{DIM}ac{RESET}");
        if s.charging == Some(true) {
            t.push_str(&format!(" {GREEN}↑{RESET}"));
        }
        if let Some(w) = s.adapter_w {
            t.push_str(&format!(" {DIM}{w}W{RESET}"));
        }
        Some(t)
    } else {
        let c = s.charge_pct.unwrap_or(0.0);
        let color = if c <= 20.0 {
            RED
        } else if c <= 40.0 {
            AMBER
        } else {
            GREEN
        };
        Some(format!(
            "{DIM}bat{RESET} {color}{}%{RESET}",
            c.round() as i64
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                for d in chars.by_ref() {
                    if d == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn base() -> State {
        State {
            fetched_at: 1000.0,
            last_attempt: 1000.0,
            ncpu: 12,
            ..Default::default()
        }
    }

    #[test]
    fn healthy_line() {
        let mut s = base();
        s.speed_limit = Some(100);
        s.load1 = Some(0.5);
        s.cpu_util = Some(12.0);
        s.mem_used_pct = Some(38.0);
        s.mem_pressure = Some(1);
        s.temp_c = Some(41.0);
        s.top_name = Some("WindowServer".into());
        s.top_cpu = Some(6.0);
        s.on_ac = Some(true);
        s.adapter_w = Some(96);
        let got = strip(&line(Some(&s), 1001.0));
        assert_eq!(
            got,
            "mach  clk 100% · load 0.5 · cpu 12% · mem 38% · 41°C · top WindowServer 6% · ac 96W"
        );
    }

    #[test]
    fn throttled_hot_day() {
        let mut s = base();
        s.speed_limit = Some(31);
        s.load1 = Some(6.5);
        s.cpu_util = Some(100.0);
        s.mem_used_pct = Some(71.0);
        s.mem_pressure = Some(2);
        s.swap_used = Some(2.4 * 1024.0 * 1024.0 * 1024.0);
        s.temp_c = Some(96.0);
        s.top_name = Some("iTerm2".into());
        s.top_cpu = Some(61.0);
        s.on_ac = Some(true);
        s.draining_on_ac = true;
        s.adapter_w = Some(96);
        let got = strip(&line(Some(&s), 1001.0));
        assert!(got.contains("clk 31%"), "{got}");
        assert!(got.contains("swap 2.4G"), "{got}");
        assert!(got.contains("ac⚠ drain 96W"), "{got}");
    }

    #[test]
    fn zero_swap_omitted_and_stale_marker() {
        let mut s = base();
        s.speed_limit = Some(100);
        s.swap_used = Some(0.0);
        let got = strip(&line(Some(&s), 1000.0 + 999.0));
        assert!(!got.contains("swap"), "zero swap should be omitted: {got}");
        assert!(got.contains("(stale)"), "{got}");
    }

    #[test]
    fn no_cache_is_quiet() {
        assert!(strip(&line(None, 1.0)).contains("gathering"));
    }

    #[test]
    fn on_battery() {
        let mut s = base();
        s.on_ac = Some(false);
        s.charge_pct = Some(64.0);
        assert!(strip(&line(Some(&s), 1001.0)).contains("bat 64%"));
    }
}
