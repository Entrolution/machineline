//! CPU speed-limit / thermal-power throttle, read from `pmset -g therm`.
//!
//! `pmset` reports `CPU_Speed_Limit = N` — the percent of rated speed the scheduler is currently
//! allowed to use. 100 is unclamped; lower means macOS is throttling the CPU (thermal headroom, a
//! VRM/port sensor, or a power-delivery limit). This is the headline machineline metric. It is
//! macOS-specific and really only meaningful on **Intel** Macs — Apple Silicon's `pmset` typically
//! omits it, so this returns `None` and the segment simply disappears.

use crate::sys::output;

/// The CPU speed limit as a percentage (100 = full speed), or `None` if unavailable.
pub fn speed_limit() -> Option<u32> {
    parse(&output("pmset", &["-g", "therm"])?)
}

/// Pull `CPU_Speed_Limit = N` out of `pmset -g therm` text. Pure, for testing.
pub fn parse(text: &str) -> Option<u32> {
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("CPU_Speed_Limit") {
            let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                return digits.parse().ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Note: No thermal warning level has been recorded\n\
        2026-06-24 18:55:24 +0100 CPU Power notify\n\
        \tCPU_Scheduler_Limit \t= 40\n\
        \tCPU_Available_CPUs \t= 12\n\
        \tCPU_Speed_Limit \t= 20\n";

    #[test]
    fn parses_speed_limit() {
        assert_eq!(parse(SAMPLE), Some(20));
        assert_eq!(parse("CPU_Speed_Limit = 100"), Some(100));
        assert_eq!(parse("no such field"), None);
    }
}
